//! Knitting machine & human hand-knit exporters (`FTX-K06`, `FTX-K07`):
//! - Verified CMU Knitout v2 (`.k`) + sealed `ftx-machine-cert/1` + `Exp-3` calibration swatch kits;
//! - Explicit Kniterate `.kc` refusal until an actual pass-oriented KCode backend is implemented;
//! - Brother KH-930 / KH-270 AYAB 1-bit indexed PNG + synchronized manual-carriage checklist;
//! - Human hand-knit pure-vector CYC SVG & PDF charts with dedicated acceptance checks;
//! - Fail-closed Tier-B proprietary refusal (`OpaqueProprietaryTarget`) for Shima/Stoll binaries.

pub mod ayab;
pub mod hand_knit;
pub mod kniterate;
pub mod knitout;
pub mod vendor_handoff;

pub use ayab::{
    AyabExportBundle, AyabMode, AyabSessionChecklist, export_brother_ayab, verify_ayab_1bit_png,
};
pub use hand_knit::{
    HandKnitExportBundle, check_hand_knit_pdf, check_hand_knit_svg, export_hand_knit_document,
};
pub use kniterate::{KniterateExportBundle, export_kniterate_kc, verify_kniterate_kc_roundtrip};
pub use knitout::{
    KnitoutExportBundle, SwatchKitBundle, export_calibration_swatch_kit, export_knitout_v2,
    verify_knitout_v2_readback,
};
pub use vendor_handoff::{
    TierBProprietaryTarget, TierBVendorHandoff, VENDOR_HANDOFF_SCHEMA, VendorHandoffManifest,
    export_tier_b_vendor_handoff, is_proprietary_handoff, refuse_proprietary_binary_target,
    verify_tier_b_handoff_bundle,
};
