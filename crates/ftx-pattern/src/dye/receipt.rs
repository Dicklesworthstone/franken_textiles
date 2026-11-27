//! Standalone sealed programmed-dyeing plan uncertainty & Four-Claims receipt (`FTX-K13`, Seam 4).
//!
//! Produced by `ftx-drape` (`optimize_yarn_dye_program_with_context`) and verified fail-closed by
//! `ftx-export` (`verify_and_export_dye_program_with_receipt`) without introducing a crate cycle
//! between `ftx-export` and `ftx-drape`.

use std::collections::BTreeSet;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::dye::ir::DyeingRegime;
pub use crate::dye::qualification::OpticalMeasurementBasis;
use crate::materials::Evidence;

/// Schema version for sealed programmed-dyeing plan uncertainty receipts (`FTX-K13`, Seam 4).
pub const DYE_UNCERTAINTY_SCHEMA: &str = "ftx-dye-uncertainty/1";

/// Open-loop stitch-phase drift and coherent horizon ledger (`FTX-K13`, Plan §4.1 / §4.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeDriftLedger {
    /// Topology shear multiplier (`1.0` for circular knitting, `2.0` for flatbed boustrophedon).
    pub topology_multiplier: f64,
    /// Systematic proportional yarn-intake bias `epsilon` (dimensionless).
    pub drift_epsilon: f64,
    /// Per-stitch stochastic Brownian jitter standard deviation `sigma_eta` (in stitches, `>= 0`).
    pub drift_sigma_eta: f64,
    /// Worst-case stitch phase shift `Delta n_max` over the full garment/swatch run (`>= 0`).
    pub max_stitch_phase_shift: f64,
    /// Active coherent open-loop horizon in whole courses for the planned topology.
    pub coherent_course_horizon: usize,
    /// Coherent open-loop circular knitting horizon in stitches.
    pub coherent_horizon_circular_stitches: usize,
    /// Coherent open-loop flatbed boustrophedon horizon in stitches.
    pub coherent_horizon_flatbed_stitches: usize,
}

impl DyeDriftLedger {
    fn validate(&self) -> Result<(), Refusal> {
        Refusal::finite("drift_ledger.topology_multiplier", self.topology_multiplier)?;
        Refusal::finite("drift_ledger.drift_epsilon", self.drift_epsilon)?;
        Refusal::finite("drift_ledger.drift_sigma_eta", self.drift_sigma_eta)?;
        Refusal::finite(
            "drift_ledger.max_stitch_phase_shift",
            self.max_stitch_phase_shift,
        )?;
        if self.topology_multiplier < 1.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "drift_ledger.topology_multiplier must be >= 1.0 (got {})",
                    self.topology_multiplier
                ),
            ));
        }
        if self.drift_sigma_eta < 0.0 || self.max_stitch_phase_shift < 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "drift_ledger.drift_sigma_eta and max_stitch_phase_shift must be non-negative",
            ));
        }
        Ok(())
    }
}

/// Capillary wicking blur floor, primary chromatographic halo, and unreproducible spatial energy
/// residual (`FTX-K13`, Plan §4.1 / §4.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeFeatureFloorSummary {
    /// Capillary wicking half-width `sigma_wick` in millimetres (`>= 0`).
    pub sigma_wick_mm: f64,
    /// Capillary wicking half-width normalized by mean loop length (`sigma_wick / ell_0`, `>= 0`).
    pub sigma_wick_stitches: f64,
    /// Minimum physically reproducible feature width `F_min` in millimetres (`>= 0`).
    pub min_reproducible_feature_mm: f64,
    /// Differential primary chromatographic fringe width `Delta x_{R_f}` in millimetres (`>= 0`).
    pub chromatographic_halo_mm: f64,
    /// Target high-frequency spatial energy fraction above applicator bandwidth (`0.0..=1.0`).
    pub unreproducible_energy_fraction: f64,
}

impl DyeFeatureFloorSummary {
    fn validate(&self) -> Result<(), Refusal> {
        for (name, val) in [
            ("feature_floor.sigma_wick_mm", self.sigma_wick_mm),
            (
                "feature_floor.sigma_wick_stitches",
                self.sigma_wick_stitches,
            ),
            (
                "feature_floor.min_reproducible_feature_mm",
                self.min_reproducible_feature_mm,
            ),
            (
                "feature_floor.chromatographic_halo_mm",
                self.chromatographic_halo_mm,
            ),
        ] {
            Refusal::finite(name, val)?;
            if val < 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{name} cannot be negative (got {val})"),
                ));
            }
        }
        Refusal::finite(
            "feature_floor.unreproducible_energy_fraction",
            self.unreproducible_energy_fraction,
        )?;
        if !(0.0..=1.0).contains(&self.unreproducible_energy_fraction) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "feature_floor.unreproducible_energy_fraction must be in [0, 1] (got {})",
                    self.unreproducible_energy_fraction
                ),
            ));
        }
        Ok(())
    }
}

/// Dressed appearance colorimetry, aspect-ratio stretch, shrinkage perturbation, and pore
/// grin-through summary (`FTX-K13`, Plan §4.4).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DressedAppearanceSummary {
    /// Predicted mean foreground CIEDE2000 perceptual error (`>= 0`).
    pub delta_e00_mean: f64,
    /// Predicted median foreground CIEDE2000 perceptual error (`>= 0`).
    pub delta_e00_p50: f64,
    /// Predicted 95th-percentile drift-robust CIEDE2000 perceptual error (`>= 0`).
    pub delta_e00_p95: f64,
    /// Irreducible substrate/dye-gamut floor CIEDE2000 at zero drift and zero wicking (`>= 0`).
    pub gamut_floor_delta_e00: f64,
    /// Upper bound on dressed stitch aspect-ratio distortion (`> 0`).
    pub aspect_ratio_stretch_bound: f64,
    /// True when wet/dry differential shrinkage exceeds the perturbation guard threshold.
    pub differential_shrinkage_stretch_perturbation_flagged: bool,
    /// Deterministic `BTreeSet`-sorted zone identifiers where pore grin-through exceeds `0.20`.
    #[serde(default)]
    pub pore_grin_through_alert_zones: Vec<String>,
}

impl DressedAppearanceSummary {
    fn validate(&self) -> Result<(), Refusal> {
        for (name, val) in [
            ("dressed_appearance.delta_e00_mean", self.delta_e00_mean),
            ("dressed_appearance.delta_e00_p50", self.delta_e00_p50),
            ("dressed_appearance.delta_e00_p95", self.delta_e00_p95),
            (
                "dressed_appearance.gamut_floor_delta_e00",
                self.gamut_floor_delta_e00,
            ),
        ] {
            Refusal::finite(name, val)?;
            if val < 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("{name} cannot be negative (got {val})"),
                ));
            }
        }
        Refusal::finite(
            "dressed_appearance.aspect_ratio_stretch_bound",
            self.aspect_ratio_stretch_bound,
        )?;
        if self.aspect_ratio_stretch_bound <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!(
                    "dressed_appearance.aspect_ratio_stretch_bound must be > 0 (got {})",
                    self.aspect_ratio_stretch_bound
                ),
            ));
        }
        Ok(())
    }
}

/// Explicit Four-Claims separation status carried on [`DyePlanUncertaintyReceipt`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeFourClaimsStatus {
    /// True ONLY when backed by calibrated non-estimate optics, non-estimate evidence, and a bound
    /// [`crate::dye::YarnDyeingQualification`]; ALWAYS `false` otherwise.
    pub color_fidelity_claimed: bool,
    /// ALWAYS `false` on computed plans until physical dyed-and-knitted swatch measurements are linked.
    pub visual_realization_swatch_verified: bool,
    /// ALWAYS `false`: computation never authorizes unattended hardware execution.
    pub hardware_execution_authorized: bool,
    /// ALWAYS `false`: computation never proves physical dyed appearance or wash fastness.
    pub physical_dyeing_validated: bool,
    /// ALWAYS `false`: real-body fit is established only by a physical muslin or dressed wearer.
    pub fit_claimed: bool,
}

/// Input payload passed to [`DyePlanUncertaintyReceipt::seal`].
#[derive(Clone, Debug, PartialEq)]
pub struct DyePlanUncertaintyInput {
    /// Canonical SHA-256 of the [`crate::dye::YarnDyeProgram`].
    pub program_sha256: String,
    /// Canonical SHA-256 of the [`crate::dye::DyeMachineProfile`].
    pub machine_sha256: String,
    /// Optional canonical SHA-256 of the bound [`crate::dye::YarnDyeingQualification`].
    pub qualification_sha256: Option<String>,
    /// Planned dyeing regime.
    pub regime: DyeingRegime,
    /// Optical measurement basis behind the Kubelka–Munk colorimetry curve.
    pub optical_measurement_basis: OpticalMeasurementBasis,
    /// Drift ledger & open-loop coherent horizons.
    pub drift_ledger: DyeDriftLedger,
    /// Capillary feature floor & spatial frequency residual.
    pub feature_floor: DyeFeatureFloorSummary,
    /// Dressed appearance, stretch, and pore grin-through summary.
    pub dressed_appearance: DressedAppearanceSummary,
    /// Evidence basis of the dye optics and transport parameters.
    pub evidence: Evidence,
    /// Additional gating/advisory flags (sorted deterministically on seal).
    pub gating_flags: Vec<String>,
}

#[derive(Serialize)]
struct DyeUncertaintySealView<'a> {
    schema: &'a str,
    program_sha256: &'a str,
    machine_sha256: &'a str,
    qualification_sha256: &'a Option<String>,
    regime: DyeingRegime,
    optical_measurement_basis: OpticalMeasurementBasis,
    drift_ledger: &'a DyeDriftLedger,
    feature_floor: &'a DyeFeatureFloorSummary,
    dressed_appearance: &'a DressedAppearanceSummary,
    four_claims: &'a DyeFourClaimsStatus,
    evidence: Evidence,
    gating_flags: &'a [String],
}

/// Sealed programmed-dyeing plan uncertainty & claim-boundary receipt (`FTX-K13`, Seam 4).
///
/// Forces `four_claims.color_fidelity_claimed = false` whenever optical curves are uncalibrated or
/// estimated (`OpticalMeasurementBasis::UnqualifiedUncalibrated` / `EstimateOnly`), whenever
/// `evidence == Evidence::Estimate`, or whenever no `qualification_sha256` is bound.
/// `visual_realization_swatch_verified`, `hardware_execution_authorized`,
/// `physical_dyeing_validated`, and `fit_claimed` are ALWAYS `false`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyePlanUncertaintyReceipt {
    /// Must equal [`DYE_UNCERTAINTY_SCHEMA`] (`"ftx-dye-uncertainty/1"`).
    pub schema: String,
    /// Canonical SHA-256 of the [`crate::dye::YarnDyeProgram`].
    pub program_sha256: String,
    /// Canonical SHA-256 of the [`crate::dye::DyeMachineProfile`].
    pub machine_sha256: String,
    /// Optional canonical SHA-256 of the bound [`crate::dye::YarnDyeingQualification`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_sha256: Option<String>,
    /// Planned dyeing regime.
    pub regime: DyeingRegime,
    /// Optical measurement basis behind the Kubelka–Munk colorimetry curve.
    pub optical_measurement_basis: OpticalMeasurementBasis,
    /// Drift ledger & open-loop coherent horizons.
    pub drift_ledger: DyeDriftLedger,
    /// Capillary feature floor & spatial frequency residual.
    pub feature_floor: DyeFeatureFloorSummary,
    /// Dressed appearance, stretch, and pore grin-through summary.
    pub dressed_appearance: DressedAppearanceSummary,
    /// Explicit Four-Claims separation status.
    pub four_claims: DyeFourClaimsStatus,
    /// Evidence basis of the dye optics and transport parameters.
    pub evidence: Evidence,
    /// Explicit gating/advisory flags (deterministically sorted via `BTreeSet`).
    #[serde(default)]
    pub gating_flags: Vec<String>,
    /// Sealed canonical SHA-256 over all fields above.
    pub payload_sha256: String,
}

impl DyePlanUncertaintyReceipt {
    /// Seals a new [`DyePlanUncertaintyReceipt`], enforcing claim boundaries, deterministic sorting
    /// of zone/gating lists, scalar bounds, and canonical SHA-256 digest sealing.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`], [`RefusalCode::NonFinite`], or [`RefusalCode::OutOfRange`].
    pub fn seal(mut input: DyePlanUncertaintyInput) -> Result<Self, Refusal> {
        if input.program_sha256.trim().is_empty() || input.machine_sha256.trim().is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "dye plan uncertainty receipt requires non-empty program_sha256 and machine_sha256",
            ));
        }
        input.drift_ledger.validate()?;
        input.feature_floor.validate()?;
        input.dressed_appearance.validate()?;

        let sorted_zones: BTreeSet<String> = input
            .dressed_appearance
            .pore_grin_through_alert_zones
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        input.dressed_appearance.pore_grin_through_alert_zones = sorted_zones.into_iter().collect();

        let color_fidelity_claimed = !input
            .optical_measurement_basis
            .is_uncalibrated_or_estimate()
            && input.evidence != Evidence::Estimate
            && input
                .qualification_sha256
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());

        let four_claims = DyeFourClaimsStatus {
            color_fidelity_claimed,
            visual_realization_swatch_verified: false,
            hardware_execution_authorized: false,
            physical_dyeing_validated: false,
            fit_claimed: false,
        };

        let mut sorted_flags: BTreeSet<String> = input
            .gating_flags
            .into_iter()
            .filter(|s| !s.trim().is_empty())
            .collect();
        if !color_fidelity_claimed {
            sorted_flags.insert("uncalibrated_optics".to_string());
        }
        if input
            .dressed_appearance
            .differential_shrinkage_stretch_perturbation_flagged
        {
            sorted_flags.insert("differential_shrinkage_stretch_perturbation".to_string());
        }
        if !input
            .dressed_appearance
            .pore_grin_through_alert_zones
            .is_empty()
        {
            sorted_flags.insert("knit_pore_grin_through".to_string());
        }
        let gating_flags: Vec<String> = sorted_flags.into_iter().collect();

        let view = DyeUncertaintySealView {
            schema: DYE_UNCERTAINTY_SCHEMA,
            program_sha256: &input.program_sha256,
            machine_sha256: &input.machine_sha256,
            qualification_sha256: &input.qualification_sha256,
            regime: input.regime,
            optical_measurement_basis: input.optical_measurement_basis,
            drift_ledger: &input.drift_ledger,
            feature_floor: &input.feature_floor,
            dressed_appearance: &input.dressed_appearance,
            four_claims: &four_claims,
            evidence: input.evidence,
            gating_flags: &gating_flags,
        };
        let payload_sha256 = canonical_json_sha256(&view)?;
        Ok(Self {
            schema: DYE_UNCERTAINTY_SCHEMA.to_string(),
            program_sha256: input.program_sha256,
            machine_sha256: input.machine_sha256,
            qualification_sha256: input.qualification_sha256,
            regime: input.regime,
            optical_measurement_basis: input.optical_measurement_basis,
            drift_ledger: input.drift_ledger,
            feature_floor: input.feature_floor,
            dressed_appearance: input.dressed_appearance,
            four_claims,
            evidence: input.evidence,
            gating_flags,
            payload_sha256,
        })
    }

    /// Verifies schema, metric bounds, Four-Claims invariants, and canonical SHA-256 seal integrity.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`], [`RefusalCode::NonFinite`], [`RefusalCode::OutOfRange`],
    /// or [`RefusalCode::VerificationMismatch`] if tampered or claiming unauthorized fidelity/validation.
    pub fn verify_integrity(&self) -> Result<(), Refusal> {
        if self.schema != DYE_UNCERTAINTY_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "expected schema '{DYE_UNCERTAINTY_SCHEMA}', got '{}'",
                    self.schema
                ),
            ));
        }
        if self.program_sha256.trim().is_empty() || self.machine_sha256.trim().is_empty() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                "dye plan uncertainty receipt requires non-empty program_sha256 and machine_sha256",
            ));
        }
        self.drift_ledger.validate()?;
        self.feature_floor.validate()?;
        self.dressed_appearance.validate()?;

        if self.four_claims.visual_realization_swatch_verified
            || self.four_claims.hardware_execution_authorized
            || self.four_claims.physical_dyeing_validated
            || self.four_claims.fit_claimed
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "computed dye plan uncertainty receipt violates Four-Claims honesty invariants (visual_realization_swatch_verified, hardware_execution_authorized, physical_dyeing_validated, and fit_claimed must all be false)",
            ));
        }
        let fidelity_allowed = !self.optical_measurement_basis.is_uncalibrated_or_estimate()
            && self.evidence != Evidence::Estimate
            && self
                .qualification_sha256
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty());
        if self.four_claims.color_fidelity_claimed && !fidelity_allowed {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "uncalibrated, estimated, or unqualified dye plan cannot claim color_fidelity_claimed = true",
            ));
        }
        let view = DyeUncertaintySealView {
            schema: &self.schema,
            program_sha256: &self.program_sha256,
            machine_sha256: &self.machine_sha256,
            qualification_sha256: &self.qualification_sha256,
            regime: self.regime,
            optical_measurement_basis: self.optical_measurement_basis,
            drift_ledger: &self.drift_ledger,
            feature_floor: &self.feature_floor,
            dressed_appearance: &self.dressed_appearance,
            four_claims: &self.four_claims,
            evidence: self.evidence,
            gating_flags: &self.gating_flags,
        };
        let expected = canonical_json_sha256(&view)?;
        if self.payload_sha256 != expected {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dye plan uncertainty receipt payload_sha256 failed tamper check",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_input(
        basis: OpticalMeasurementBasis,
        evidence: Evidence,
        qualification_sha256: Option<String>,
        drift_epsilon: f64,
    ) -> DyePlanUncertaintyInput {
        DyePlanUncertaintyInput {
            program_sha256: "prog-sha-1".to_string(),
            machine_sha256: "mach-sha-1".to_string(),
            qualification_sha256,
            regime: DyeingRegime::Regime1AStripes,
            optical_measurement_basis: basis,
            drift_ledger: DyeDriftLedger {
                topology_multiplier: 2.0,
                drift_epsilon,
                drift_sigma_eta: 0.015,
                max_stitch_phase_shift: 1.4,
                coherent_course_horizon: 10,
                coherent_horizon_circular_stitches: 180,
                coherent_horizon_flatbed_stitches: 60,
            },
            feature_floor: DyeFeatureFloorSummary {
                sigma_wick_mm: 1.5,
                sigma_wick_stitches: 0.22,
                min_reproducible_feature_mm: 3.3,
                chromatographic_halo_mm: 0.3,
                unreproducible_energy_fraction: 0.08,
            },
            dressed_appearance: DressedAppearanceSummary {
                delta_e00_mean: 1.4,
                delta_e00_p50: 1.2,
                delta_e00_p95: 2.4,
                gamut_floor_delta_e00: 0.5,
                aspect_ratio_stretch_bound: 1.18,
                differential_shrinkage_stretch_perturbation_flagged: false,
                pore_grin_through_alert_zones: vec![
                    "course_0004_pore_gt_0.20".to_string(),
                    "course_0001_pore_gt_0.20".to_string(),
                ],
            },
            evidence,
            gating_flags: Vec::new(),
        }
    }

    #[test]
    fn receipt_seals_verifies_roundtrip_canonicalizes_neg_zero_and_rejects_tamper() {
        let neg_zero = DyePlanUncertaintyReceipt::seal(sample_input(
            OpticalMeasurementBasis::CalibratedChartScanner,
            Evidence::Measured,
            Some("qual-sha".to_string()),
            -0.0,
        ))
        .unwrap();
        let pos_zero = DyePlanUncertaintyReceipt::seal(sample_input(
            OpticalMeasurementBasis::CalibratedChartScanner,
            Evidence::Measured,
            Some("qual-sha".to_string()),
            0.0,
        ))
        .unwrap();
        assert_eq!(neg_zero.payload_sha256, pos_zero.payload_sha256);
        assert!(pos_zero.four_claims.color_fidelity_claimed);
        assert!(!pos_zero.four_claims.visual_realization_swatch_verified);
        assert!(!pos_zero.four_claims.hardware_execution_authorized);
        assert!(!pos_zero.four_claims.physical_dyeing_validated);
        assert!(!pos_zero.four_claims.fit_claimed);
        assert_eq!(
            pos_zero.dressed_appearance.pore_grin_through_alert_zones,
            vec![
                "course_0001_pore_gt_0.20".to_string(),
                "course_0004_pore_gt_0.20".to_string()
            ]
        );
        pos_zero.verify_integrity().unwrap();

        // JSON round-trip & deny_unknown_fields on receipt + sub-structs
        let json_str = serde_json::to_string(&pos_zero).unwrap();
        let rt: DyePlanUncertaintyReceipt = serde_json::from_str(&json_str).unwrap();
        assert_eq!(rt, pos_zero);

        let mut val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        val.as_object_mut()
            .unwrap()
            .insert("unknown".to_string(), serde_json::json!(true));
        assert!(serde_json::from_value::<DyePlanUncertaintyReceipt>(val).is_err());

        let mut sub_val: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        sub_val["drift_ledger"]
            .as_object_mut()
            .unwrap()
            .insert("unknown".to_string(), serde_json::json!(1));
        assert!(serde_json::from_value::<DyePlanUncertaintyReceipt>(sub_val).is_err());

        // UnqualifiedUncalibrated, EstimateOnly, Evidence::Estimate, and missing qualification_sha256
        // all force color_fidelity_claimed = false.
        for (basis, ev, q) in [
            (
                OpticalMeasurementBasis::UnqualifiedUncalibrated,
                Evidence::Measured,
                Some("qual-sha".to_string()),
            ),
            (
                OpticalMeasurementBasis::EstimateOnly,
                Evidence::Measured,
                Some("qual-sha".to_string()),
            ),
            (
                OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
                Evidence::Estimate,
                Some("qual-sha".to_string()),
            ),
            (
                OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
                Evidence::Measured,
                None,
            ),
        ] {
            let r = DyePlanUncertaintyReceipt::seal(sample_input(basis, ev, q, 0.001)).unwrap();
            assert!(!r.four_claims.color_fidelity_claimed);
            assert!(r.gating_flags.iter().any(|f| f == "uncalibrated_optics"));
            r.verify_integrity().unwrap();

            let mut forged = r.clone();
            forged.four_claims.color_fidelity_claimed = true;
            assert_eq!(
                forged.verify_integrity().unwrap_err().code,
                RefusalCode::VerificationMismatch
            );
        }

        // Tampering any Four-Claims false boolean or metric fails with VerificationMismatch
        for mutator in [
            (|r: &mut DyePlanUncertaintyReceipt| {
                r.four_claims.visual_realization_swatch_verified = true;
            }) as fn(&mut DyePlanUncertaintyReceipt),
            |r| r.four_claims.hardware_execution_authorized = true,
            |r| r.four_claims.physical_dyeing_validated = true,
            |r| r.four_claims.fit_claimed = true,
            |r| r.dressed_appearance.delta_e00_p95 = 0.01,
            |r| r.drift_ledger.max_stitch_phase_shift = 0.0,
            |r| r.feature_floor.sigma_wick_mm = 0.1,
        ] {
            let mut tampered = pos_zero.clone();
            mutator(&mut tampered);
            assert_eq!(
                tampered.verify_integrity().unwrap_err().code,
                RefusalCode::VerificationMismatch
            );
        }

        // NaN & out-of-range checks
        let mut nan_in = sample_input(
            OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
            Evidence::Measured,
            Some("qual-sha".to_string()),
            0.0,
        );
        nan_in.feature_floor.sigma_wick_mm = f64::NAN;
        assert_eq!(
            DyePlanUncertaintyReceipt::seal(nan_in).unwrap_err().code,
            RefusalCode::NonFinite
        );

        let mut oor_in = sample_input(
            OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
            Evidence::Measured,
            Some("qual-sha".to_string()),
            0.0,
        );
        oor_in.dressed_appearance.aspect_ratio_stretch_bound = 0.0;
        assert_eq!(
            DyePlanUncertaintyReceipt::seal(oor_in).unwrap_err().code,
            RefusalCode::OutOfRange
        );
    }
}
