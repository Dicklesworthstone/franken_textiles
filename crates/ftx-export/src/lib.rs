//! FrankenTextiles exports (`docs/spec/03`): every deliverable as a projection of one pattern. So far the
//! text and number helpers (§2), the seam-allowance cut outline (§3–4), piece geometry (§5), layouts (§6), the
//! master SVG (§7.1) and the PDFs (§7.2–7.6).
//!
//! Frame: millimetres, the pattern's own frame (x right, y up, loops counter-clockwise).
#![forbid(unsafe_code)]

pub mod all;
pub mod checks;
pub mod ctx;
pub mod cutter;
pub mod cutting;
pub mod dst;
pub mod dxf;
pub mod gc;
pub mod geom;
pub mod hpgl;
pub mod layout;
pub mod outline;
pub mod pdf;
pub mod pole;
pub mod preview;
pub mod pyjson;
pub mod svg;
pub mod text;
pub mod union;
mod xml;

pub use all::{ExportSet, Target, check_files, export_all, export_targets, read_dir_files};
pub use checks::{
    CheckItem, check_dst, check_dxf, check_flat_dxf, check_garmentcode, check_hpgl, check_pdf,
    check_svg, pdf_objects, svg_cut_points,
};
pub use ctx::{Ctx, ExportOptions, primary_instance};
pub use cutter::{CutterFile, export_cutter};
pub use dst::{
    Block, COUCH_STITCH_MM, DST_END, DstFile, DstInfo, DstRead, PLACE_STITCH_MM, dst_decode,
    dst_record, export_embroidery, read_dst, write_dst,
};
pub use dxf::{export_dxf_astm, export_flat_dxf, turn_and_curve_points};
pub use gc::export_garmentcode;
pub use geom::{
    Affine, CAP, Decor, FoldMode, Geom, GeomMeta, Label, Notch, Source, build_geom, grain_marks,
    grain_vertical_rotation, label_texts, main_label_fits, notch_depth,
};
pub use hpgl::export_hpgl;
pub use layout::{Layout, rotation, shelf_layout};
pub use outline::{
    DEFAULT_SA, EdgeLoop, Method, clean_ring, cut_outline, drafted_instance, edge_loops,
    edge_points, edge_sa, ring_is_simple, sew_breaks, sew_ring,
};
pub use pdf::{
    Canvas, INCH, OCG_COMMON, PT_PER_MM, Paper, PdfDoc, PdfFile, Tile, export_copyshop,
    export_projector, export_tiled, tile_grid,
};
pub use preview::diagnostic_svg;
pub use pyjson::PyJson;
pub use svg::{SVG_NAMESPACE, master_svg, svg_piece_group};
pub use text::{
    Anchor, ascii_text, column_letters, fnum, page_list, pdf_string, safe_name, stroke_text,
    text_width, wrap, xml_escape,
};
