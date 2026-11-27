//! Knitting pattern IR (`ftx-knit/1`), lossless CMU Knitout v2 AST, machine profiles,
//! 3D-to-knit shaping seam (`ShapingSurface`), and sealed qualification/certification receipts.
//!
//! All types here are pure data and validation logic with zero dependency on `ftx-body`,
//! `ftx-drape`, `ftx-trace`, or `ftx-export`, preserving the 10-crate acyclic DAG.

pub mod draft;
pub mod ir;
pub mod profile;
pub mod receipts;
pub mod shaping;

pub use draft::{
    ParametricPanelSpec, RaglanPulloverSpec, draft_calibration_swatch, draft_seamless_raglan_tube,
    draft_shaped_panel,
};

pub use ir::{
    Bed, CableGroup, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA,
    KNITOUT_SCHEMA, KnitDoc, KnitoutHeader, KnitoutOp, KnitoutProgram, LoopKind, Needle,
    ParentRef, ShortRowTurn, StitchNode, YarnRun,
};
pub use profile::{CalibrationSwatchCaps, MachineBackendKind, MachineProfile, WearerAgeClass};
pub use receipts::{
    DRAPE_UNCERTAINTY_SCHEMA, DrapeUncertaintyReceipt, FourClaimsVerdict, KNIT_CERT_SCHEMA,
    KnitMachineCert, KnitSwatchQualification,
};
pub use shaping::{BoundaryRing, EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface};
