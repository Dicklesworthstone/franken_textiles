//! Knitting pattern IR (`ftx-knit/1`), lossless CMU Knitout v2 AST, machine profiles,
//! 3D-to-knit shaping seam (`ShapingSurface`), and sealed qualification/certification receipts.
//!
//! All types here are pure data and validation logic with zero dependency on `ftx-body`,
//! `ftx-drape`, `ftx-trace`, or `ftx-export`, preserving the 10-crate acyclic DAG.

pub mod autoknit;
pub mod compile;
pub mod draft;
pub mod garment;
pub mod ir;
pub mod lifted_procir;
pub mod lifted_tangle;
pub mod profile;
pub mod receipts;
pub mod shaping;
pub mod vm;

pub use autoknit::{AutoKnitConfig, AutoKnitReport, draft_autoknit_from_shell};
pub use compile::{CompiledKnitBundle, compile_knit_doc};
pub use draft::{
    BranchedPulloverSpec, ParametricPanelSpec, RaglanPulloverSpec, apply_cable_crossings,
    draft_branched_raglan_pullover, draft_calibration_swatch, draft_seamless_raglan_tube,
    draft_shaped_panel, draft_short_row_neckline_panel,
};
pub use garment::{
    AssemblySeam, GarmentEdge, GarmentEdgeKind, GarmentOpening, PulloverPiece, PulloverPieceRole,
    SEAMED_PULLOVER_SCHEMA, SeamJoinGroup, SeamedPulloverDraft, SeamedPulloverSpec,
    draft_seamed_pullover,
};
pub use vm::{
    KnitSafetyContext, RackingStrainReport, RackingStrainSpec, VBedVmReport,
    synthesize_regime3_intake_timeline, verify_knitout_program,
    verify_knitout_program_with_racking_strain, verify_racking_strain,
};

pub use ir::{
    Bed, CableGroup, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA,
    KNITOUT_SCHEMA, KnitDoc, KnitPass, KnitoutHeader, KnitoutOp, KnitoutProgram, LoopKind, Needle,
    ParentRef, ShortRowTurn, StitchNode, YarnRun, format_quarter_pitches, parse_knitout,
};
pub use profile::{
    CalibrationSwatchCaps, MachineBackendKind, MachineProfile, MachineTimingKinematics,
    WearerAgeClass,
};
pub use receipts::{
    DRAPE_UNCERTAINTY_SCHEMA, DrapeUncertaintyReceipt, FourClaimsVerdict, KNIT_CERT_SCHEMA,
    KnitMachineCert, KnitSwatchQualification,
};
pub use shaping::{BoundaryRing, EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface};
