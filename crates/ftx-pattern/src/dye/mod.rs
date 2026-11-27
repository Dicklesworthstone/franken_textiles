//! Programmed continuous yarn dyeing IR (`ftx-dye-program/1`), dye-rig hardware profiles,
//! and physical dyeing qualification receipts (`YarnDyeingQualification`).
//!
//! Pure data and validation contracts in `ftx-pattern` (zero solver or raster-decoder dependency).

pub mod ir;
pub mod machine;
pub mod qualification;

pub use ir::{
    DYE_PROGRAM_SCHEMA, DyeSegment, DyeingRegime, IntakeEvent, IntakeEventKind,
    Regime3IntakeTimeline, YarnDyeProgram,
};
pub use machine::{
    CapstanPinSpec, DancerAccumulatorSpec, DyeApplicatorKind, DyeMachineProfile, DyePrimarySpec,
};
pub use qualification::{OpticalMeasurementBasis, YarnDyeingQualification};
