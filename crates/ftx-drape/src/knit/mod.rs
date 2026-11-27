//! Multiscale knit physics in `ftx-drape` (`FTX-K09`, `FTX-K10`):
//! - Rung 2 homogenized knit shell constitutive extensions (`CoupledKnitMembrane` with jamming
//!   barrier & `OutOfRange` gating, $SO(3)$-objective Cayley–Hamilton $\mathbf{C}^{-1/2}$ +
//!   twice-integrated $C^3$-smooth strictly convex asymmetric bending, parallel additive
//!   $C^2$-Huberized Prandtl–Ishlinskii hysteresis, 5 `C011` honesty flags, and Seam-4 receipt);
//! - Rung 3 Poincloux et al. (2018) planar elastica knitted-sheet analytical KAT oracle;
//! - Offline RVE surrogate card bridge (`Evidence::Estimate` / `MODEL_FIT`) & local audit patches.

pub mod constitutive;
pub mod poincloux;

pub use constitutive::{
    AsymmetricKnitBending, CoupledKnitMembrane, KnitBendingEval, KnitHonestyFlag,
    KnitHysteresisBank, KnitHysteresisEval, KnitHysteresisState, KnitMembraneEval,
    KnitShellAssessment, assess_knit_shell_state, cayley_hamilton_inv_sqrt_2x2,
};
pub use poincloux::{
    KnitPriorCalibrationResult, LocalAuditPatchInput, LocalAuditPatchResult, PoinclouxParams,
    PoinclouxPoint, RveHomogenizedKnitCard, audit_local_knit_patch,
    calibrate_knit_prior_to_uniaxial_strips, poincloux_uniaxial_curve,
};
