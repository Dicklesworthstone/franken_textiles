//! Programmed continuous yarn dyeing forward physics, exact discrete adjoints, Sobol-CVaR
//! Bertsekas Two-Metric Projected L-BFGS inverse optimizer, and 5-regime Pareto selector
//! (`FTX-K14`, `FTX-K15`).
//!
//! Strictly enforces linear radiometric CIE XYZ spatial mixing (yarn segments + draped pore
//! grin-through) BEFORE nonlinear CIE `L*a*b*` / `C^infty` smooth CIEDE2000 perceptual loss.

pub mod inverse;
pub mod optics;
pub mod transport;

pub use inverse::{
    DriftParameters, DyeInverseConfig, DyeInverseResult, DyeRegimeScore, KnitStitchMap,
    StitchArcMapping, coherent_open_loop_horizons, optimize_yarn_dye_program,
    rank_five_regime_pareto,
};
pub use optics::{
    CieLab, CieXyz, DyeOpticsModel, PrimarySpectralCard, ciede2000_exact, ciede2000_sq_smooth,
    ciede2000_sq_smooth_grad, kubelka_munk_finite_slab, kubelka_munk_r_inf, linear_xyz_to_lab,
    saunderson_correct, srgb_u8_to_xyz_d65, xyz_d65_to_srgb_u8,
};
pub use transport::{
    CapillaryTransportConfig, CapillaryTransportOutput, forward_capillary_transport,
    vjp_capillary_transport,
};
