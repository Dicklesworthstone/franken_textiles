//! Knitting machine & human hand-knit exporters (`FTX-K06`, `FTX-K07`):
//! - Verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` + `Exp-3` calibration swatch kits;
//! - Kniterate 7E `.kc` emitter (`knitout-backend-kniterate` open dialect);
//! - Brother KH-930 / KH-270 AYAB 1-bit indexed PNG + synchronized manual-carriage checklist;
//! - Human hand-knit pure-vector CYC SVG & PDF charts with dedicated acceptance checks;
//! - Fail-closed Tier-B proprietary refusal (`OpaqueProprietaryTarget`) for Shima/Stoll binaries.

pub mod ayab;
pub mod hand_knit;
pub mod kniterate;
pub mod knitout;

pub use ayab::{AyabExportBundle, AyabMode, AyabSessionChecklist, export_brother_ayab};
pub use hand_knit::{
    HandKnitExportBundle, check_hand_knit_pdf, check_hand_knit_svg, export_hand_knit_document,
};
pub use kniterate::{KniterateExportBundle, export_kniterate_kc};
pub use knitout::{
    KnitoutExportBundle, SwatchKitBundle, export_calibration_swatch_kit, export_knitout_v2,
    refuse_proprietary_binary_target,
};
