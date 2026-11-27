//! Physical yarn-dyeing qualification records (Exp-4 bleed, shrinkage, tenacity & fastness gates).

use std::collections::BTreeMap;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::knit::WearerAgeClass;
use crate::materials::{Evidence, Source};

/// Optical measurement basis used to calibrate dye colorimetry curves (prevents uncalibrated sRGB claims).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OpticalMeasurementBasis {
    /// Benchtop or handheld spectrophotometer under CIE D65 / 10° observer.
    SpectrophotometerD65_10Deg,
    /// Spectrophotometer under CIE D65 / 2° observer.
    SpectrophotometerD65_2Deg,
    /// ColorChecker-calibrated sRGB camera measurement (lower accuracy than spectrophotometer).
    CalibratedCameraSrgb,
    /// Unmeasured numerical estimate (requires explicit `allow_estimates = true`).
    EstimateOnly,
}

/// Qualified physical dye-yarn-machine coupon observation (`Exp-4`).
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct YarnDyeingQualification {
    /// Stable qualification identifier.
    pub id: String,
    /// Exact yarn material digest.
    pub material_sha256: String,
    /// Exact [`super::machine::DyeMachineProfile::canonical_sha256`] digest.
    pub dye_machine_sha256: String,
    /// Physical yarn lot identifier.
    pub yarn_lot_id: String,
    /// Physical dye batch identifier.
    pub dye_lot_id: String,
    /// Measured capillary wicking half-width `sigma_wick` at nominal line speed, in mm (`>= 0`).
    pub wick_dispersion_mm: f64,
    /// Measured axial yarn shrinkage from wet-dyed to dry-relaxed state, in percent (e.g. `2.5`).
    pub shrinkage_axial_pct: f64,
    /// Ratio of wet tensile strength to dry tensile strength in `(0, 1.5]` (e.g. `0.55` for viscose/rayon, `1.05` for cotton).
    pub wet_tenacity_retention_ratio: f64,
    /// Dry yarn breaking force in Newtons (`> 0`).
    pub yarn_breaking_force_n: f64,
    /// Optional thermoplastic glass-transition / softening temperature `T_g`, in Celsius.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yarn_tg_c: Option<f64>,
    /// Optical measurement basis used to fit primary `K/S` response.
    pub optical_measurement_basis: OpticalMeasurementBasis,
    /// True only if post-dye washing-off / soap-boil clearance was verified.
    pub washing_off_validated: bool,
    /// Optional ISO 105-C06 domestic wash colorfastness grade (`1.0..=5.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iso_105_c06_grade: Option<f64>,
    /// Optional ISO 105-X12 wet rubbing fastness grade (`1.0..=5.0`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iso_105_x12_wet_rub_grade: Option<f64>,
    /// True only if perspiration/saliva fastness was validated (required for infants `< 3 yr`).
    #[serde(default)]
    pub saliva_fastness_validated: bool,
    /// Evidence basis (`Measured`, `Published`, `Estimate`).
    pub evidence: Evidence,
    /// Source citation or local lab coupon record.
    pub source: Source,
    /// Dyeing, fixation, washing-off, and measurement protocol.
    pub method: String,
    /// Ambient/oven conditions and liquor pH/additives.
    #[serde(default)]
    pub conditioning: BTreeMap<String, String>,
    /// Explicit opt-in required to consume `Evidence::Estimate` records.
    #[serde(default)]
    pub allow_estimates: bool,
}

impl YarnDyeingQualification {
    /// Validates this dyeing qualification against active hashes and wearer age safety gates.
    ///
    /// # Errors
    /// [`RefusalCode::VerificationMismatch`], [`RefusalCode::UnqualifiedProcess`], or
    /// [`RefusalCode::MaterialProcessHazard`].
    pub fn validate_for_wearer(
        &self,
        wearer: WearerAgeClass,
        expected_material_sha256: &str,
        expected_machine_sha256: &str,
    ) -> Result<(), Refusal> {
        if self.material_sha256 != expected_material_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dyeing qualification material_sha256 mismatch",
            ));
        }
        if self.dye_machine_sha256 != expected_machine_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "dyeing qualification dye_machine_sha256 mismatch",
            ));
        }
        if (self.evidence == Evidence::Estimate
            || self.optical_measurement_basis == OpticalMeasurementBasis::EstimateOnly)
            && !self.allow_estimates
        {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                "dyeing qualification is estimated without explicit allow_estimates = true",
            ));
        }
        for (k, v) in [
            ("wick_dispersion_mm", self.wick_dispersion_mm),
            ("shrinkage_axial_pct", self.shrinkage_axial_pct),
            (
                "wet_tenacity_retention_ratio",
                self.wet_tenacity_retention_ratio,
            ),
            ("yarn_breaking_force_n", self.yarn_breaking_force_n),
        ] {
            Refusal::finite(k, v)?;
        }
        if self.wick_dispersion_mm < 0.0
            || self.wet_tenacity_retention_ratio <= 0.0
            || self.wet_tenacity_retention_ratio > 1.5
            || self.yarn_breaking_force_n <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "invalid wick dispersion, wet tenacity retention, or breaking force",
            ));
        }
        if let Some(tg) = self.yarn_tg_c {
            Refusal::finite("yarn_tg_c", tg)?;
        }
        if let Some(grade) = self.iso_105_c06_grade {
            Refusal::finite("iso_105_c06_grade", grade)?;
            if !(1.0..=5.0).contains(&grade) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("iso_105_c06_grade must be in [1.0, 5.0] (got {grade})"),
                ));
            }
        }
        if let Some(grade) = self.iso_105_x12_wet_rub_grade {
            Refusal::finite("iso_105_x12_wet_rub_grade", grade)?;
            if !(1.0..=5.0).contains(&grade) {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("iso_105_x12_wet_rub_grade must be in [1.0, 5.0] (got {grade})"),
                ));
            }
        }
        if wearer.is_child() {
            if !self.washing_off_validated {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    "child/infant dyed garment requires washing_off_validated = true",
                )
                .repair("complete post-fixation soap wash-off and record clear rinse test"));
            }
            let wash_grade = self.iso_105_c06_grade.unwrap_or(0.0);
            if wash_grade < 4.0 {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "child/infant dyed garment requires ISO 105-C06 wash fastness >= 4.0 (got {wash_grade})"
                    ),
                ));
            }
        }
        if wearer.is_infant() {
            if !self.saliva_fastness_validated {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    "infant (<3 yr) dyed garment requires saliva_fastness_validated = true",
                ));
            }
            let wet_rub = self.iso_105_x12_wet_rub_grade.unwrap_or(0.0);
            if wet_rub < 4.0 {
                return Err(Refusal::new(
                    RefusalCode::MaterialProcessHazard,
                    format!(
                        "infant (<3 yr) dyed garment requires ISO 105-X12 wet rub fastness >= 4.0 (got {wet_rub})"
                    ),
                ));
            }
        }
        Ok(())
    }

    /// Canonical SHA-256 digest of this qualification record.
    ///
    /// # Errors
    /// Same as [`canonical_json_sha256`].
    pub fn canonical_sha256(&self) -> Result<String, Refusal> {
        canonical_json_sha256(self)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dye::ir::{
        DYE_PROGRAM_SCHEMA, DyeSegment, DyeingRegime, IntakeEvent, IntakeEventKind,
        Regime3IntakeTimeline, YarnDyeProgram,
    };
    use crate::dye::machine::DyeMachineProfile;

    #[test]
    fn dye_machine_and_program_enforce_dwell_and_regime3_timeline() {
        let prof = DyeMachineProfile::benchtop_piezo_cmyk();
        prof.validate().unwrap();
        let mhash = prof.canonical_sha256().unwrap();

        let mut bad_prof = prof.clone();
        bad_prof.min_line_speed_m_s = 0.01; // 0.6 / 0.01 = 60s > 20s scorch!
        assert_eq!(
            bad_prof.validate().unwrap_err().code,
            RefusalCode::MaterialProcessHazard
        );

        let mut prog = YarnDyeProgram {
            schema: DYE_PROGRAM_SCHEMA.to_string(),
            name: "stripe-test".to_string(),
            run_id: 0,
            yarn_id: "cotton-120tex".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            regime: DyeingRegime::Regime3InlineDancer,
            machine_sha256: mhash,
            qualification_sha256: None,
            startup_leader_m: 1.5,
            shutdown_trailer_m: 0.5,
            segments: vec![
                DyeSegment {
                    s_start_m: 0.0,
                    s_end_m: 2.0,
                    target_speed_m_s: 0.08,
                    doses: vec![0.5, 0.0, 0.2, 0.0],
                    fiducial_mark: false,
                },
                DyeSegment {
                    s_start_m: 2.0,
                    s_end_m: 4.0,
                    target_speed_m_s: 0.08,
                    doses: vec![0.0, 0.7, 0.1, 0.0],
                    fiducial_mark: false,
                },
            ],
            regime3_intake_timeline: None,
            expected_delta_e00_p50: Some(1.8),
            expected_delta_e00_p95: Some(3.1),
            gamut_floor_delta_e00: Some(0.9),
            unreproducible_energy_fraction: Some(0.05),
            not_for_garment_wear: false,
            metadata: BTreeMap::new(),
        };
        assert_eq!(
            prog.validate().unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        prog.regime3_intake_timeline = Some(Regime3IntakeTimeline {
            events: vec![
                IntakeEvent {
                    duration_s: 20.0,
                    yarn_intake_speed_m_s: 0.08,
                    kind: IntakeEventKind::KnitPass,
                },
                IntakeEvent {
                    duration_s: 1.5,
                    yarn_intake_speed_m_s: 0.0,
                    kind: IntakeEventKind::ReversalDwell,
                },
            ],
            dt_verify_s: 0.05,
        });
        prog.validate().unwrap();
    }

    #[test]
    fn infant_dye_qualification_refuses_unwashed_or_low_fastness() {
        let q = YarnDyeingQualification {
            id: "dq-1".to_string(),
            material_sha256: "mat".to_string(),
            dye_machine_sha256: "dm".to_string(),
            yarn_lot_id: "yl-1".to_string(),
            dye_lot_id: "dl-1".to_string(),
            wick_dispersion_mm: 1.4,
            shrinkage_axial_pct: 2.1,
            wet_tenacity_retention_ratio: 0.92,
            yarn_breaking_force_n: 8.5,
            yarn_tg_c: None,
            optical_measurement_basis: OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
            washing_off_validated: true,
            iso_105_c06_grade: Some(4.5),
            iso_105_x12_wet_rub_grade: Some(4.5),
            saliva_fastness_validated: false,
            evidence: Evidence::Measured,
            source: Source {
                reference: "local:exp4".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "coupon".to_string(),
            },
            method: "reactive steam fix + boil wash".to_string(),
            conditioning: BTreeMap::new(),
            allow_estimates: false,
        };
        q.validate_for_wearer(WearerAgeClass::Child3To14, "mat", "dm")
            .unwrap();
        assert_eq!(
            q.validate_for_wearer(WearerAgeClass::InfantUnder3, "mat", "dm")
                .unwrap_err()
                .code,
            RefusalCode::MaterialProcessHazard
        );

        // NaN / out-of-range ISO fastness grades must never silently bypass child/infant gates.
        let mut nan_wash = q.clone();
        nan_wash.iso_105_c06_grade = Some(f64::NAN);
        assert_eq!(
            nan_wash
                .validate_for_wearer(WearerAgeClass::Child3To14, "mat", "dm")
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
        let mut nan_rub = q.clone();
        nan_rub.saliva_fastness_validated = true;
        nan_rub.iso_105_x12_wet_rub_grade = Some(f64::NAN);
        assert_eq!(
            nan_rub
                .validate_for_wearer(WearerAgeClass::InfantUnder3, "mat", "dm")
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
        let mut bad_grade = q;
        bad_grade.iso_105_c06_grade = Some(6.0);
        assert_eq!(
            bad_grade
                .validate_for_wearer(WearerAgeClass::Child3To14, "mat", "dm")
                .unwrap_err()
                .code,
            RefusalCode::OutOfRange
        );
    }

    #[test]
    fn dye_machine_and_program_reject_nan_dwell_capstan_and_metric_bypasses() {
        let prof = DyeMachineProfile::benchtop_piezo_cmyk();
        let mut nan_dwell = prof.clone();
        nan_dwell.primaries[0].min_fixation_dwell_s = f64::NAN;
        assert_eq!(
            nan_dwell.validate().unwrap_err().code,
            RefusalCode::NonFinite
        );

        let mut nan_dose = prof.clone();
        nan_dose.primaries[0].max_dose_per_m = f64::NAN;
        assert_eq!(
            nan_dose.validate().unwrap_err().code,
            RefusalCode::NonFinite
        );

        let mut nan_pin = prof.clone();
        nan_pin.guide_capstan_pins[0].mu_friction = f64::NAN;
        assert_eq!(nan_pin.validate().unwrap_err().code, RefusalCode::NonFinite);

        let mut neg_pin = prof;
        neg_pin.guide_capstan_pins[0].wrap_angle_rad = -0.1;
        assert_eq!(
            neg_pin.validate().unwrap_err().code,
            RefusalCode::OutOfRange
        );

        let mut prog = YarnDyeProgram {
            schema: DYE_PROGRAM_SCHEMA.to_string(),
            name: "stripe".to_string(),
            run_id: 0,
            yarn_id: "cotton".to_string(),
            wearer_age_class: WearerAgeClass::Adult,
            regime: DyeingRegime::Regime1AStripes,
            machine_sha256: "m".to_string(),
            qualification_sha256: None,
            startup_leader_m: 1.0,
            shutdown_trailer_m: 0.5,
            segments: vec![DyeSegment {
                s_start_m: 0.0,
                s_end_m: 1.0,
                target_speed_m_s: 0.08,
                doses: vec![0.5],
                fiducial_mark: false,
            }],
            regime3_intake_timeline: None,
            expected_delta_e00_p50: Some(f64::NAN),
            expected_delta_e00_p95: None,
            gamut_floor_delta_e00: None,
            unreproducible_energy_fraction: None,
            not_for_garment_wear: false,
            metadata: BTreeMap::new(),
        };
        assert_eq!(prog.validate().unwrap_err().code, RefusalCode::NonFinite);
        prog.expected_delta_e00_p50 = Some(1.0);
        prog.unreproducible_energy_fraction = Some(1.5);
        assert_eq!(prog.validate().unwrap_err().code, RefusalCode::OutOfRange);
    }
}
