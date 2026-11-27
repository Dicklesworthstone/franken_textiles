//! Seam 4: sealed knit swatch qualifications, drape uncertainty receipts, and `ftx-machine-cert/1`.
//!
//! Enforces the Four Claims constitution: computed certifications NEVER authorize unattended
//! hardware execution (`hardware_execution_authorized: false`), NEVER claim physical knitting
//! validation (`physical_knitting_validated: false`), and NEVER claim real-body fit (`fit_claimed: false`).

use std::collections::BTreeMap;

use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::materials::{Evidence, Source};

use super::profile::WearerAgeClass;

/// Schema version for sealed drape uncertainty receipts passed to `ftx-export`.
pub const DRAPE_UNCERTAINTY_SCHEMA: &str = "ftx-drape-uncertainty/1";
/// Schema version for knitting machine certification receipts.
pub const KNIT_CERT_SCHEMA: &str = "ftx-machine-cert/1";

/// Physical swatch qualification record binding one exact yarn lot and machine profile.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitSwatchQualification {
    /// Stable qualification observation identifier.
    pub id: String,
    /// Exact source-bearing yarn/material digest.
    pub material_sha256: String,
    /// Exact [`super::profile::MachineProfile::canonical_sha256`] digest.
    pub machine_sha256: String,
    /// Physical yarn dye-lot / spool batch identifier.
    pub yarn_lot_id: String,
    /// Must be true for production garment sizing (unwashed swatches shrink/relax unpredictably).
    pub was_washed_and_blocked: bool,
    /// Measured horizontal stitch count per 100 mm.
    pub wales_per_100mm: f64,
    /// Measured vertical course count per 100 mm.
    pub courses_per_100mm: f64,
    /// Measured relaxed fabric thickness in mm.
    pub thickness_mm: f64,
    /// Dimensionless cam stitch setting used to knit the swatch.
    pub cam_stitch_setting: f64,
    /// Optional calibrated capillary wicking dispersion half-width (`sigma_wick`), in mm (Exp-4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wick_dispersion_mm: Option<f64>,
    /// Optional axial yarn shrinkage from wet-dyed to dry-blocked state, in percent (Exp-4).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shrinkage_axial_pct: Option<f64>,
    /// Optional systematic yarn intake drift fraction `epsilon` measured on Exp-3 swatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yarn_intake_drift_eps: Option<f64>,
    /// Optional per-stitch stochastic intake jitter `sigma_eta` (in stitches) measured on Exp-3 swatch.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub yarn_intake_drift_sigma_eta: Option<f64>,
    /// Origin of this observation (`Measured` / `Published` / `Estimate`).
    pub evidence: Evidence,
    /// Citation or local swatch observation metadata.
    pub source: Source,
    /// Test and washing/blocking protocol description.
    pub method: String,
    /// Temperature, RH%, steam/blocking weights, and specimen notes.
    #[serde(default)]
    pub conditioning: BTreeMap<String, String>,
    /// Separate explicit opt-in required to consume `Evidence::Estimate` records.
    #[serde(default)]
    pub allow_estimates: bool,
}

impl KnitSwatchQualification {
    /// Validates this swatch qualification for production garment compilation against expected hashes.
    ///
    /// # Errors
    /// [`RefusalCode::UnqualifiedProcess`] if unwashed, estimate without opt-in, or invalid numbers;
    /// [`RefusalCode::VerificationMismatch`] if `material_sha256` or `machine_sha256` disagrees.
    pub fn validate_for_production(
        &self,
        expected_material_sha256: &str,
        expected_machine_sha256: &str,
    ) -> Result<(), Refusal> {
        if self.material_sha256 != expected_material_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "swatch qualification '{}' material_sha256 '{}' does not match active yarn '{}'",
                    self.id, self.material_sha256, expected_material_sha256
                ),
            )
            .repair("knit and measure a calibration swatch with the exact target yarn lot"));
        }
        if self.machine_sha256 != expected_machine_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "swatch qualification '{}' machine_sha256 '{}' does not match target machine '{}'",
                    self.id, self.machine_sha256, expected_machine_sha256
                ),
            )
            .repair("re-qualify the swatch on the target machine profile"));
        }
        if self.evidence == Evidence::Estimate && !self.allow_estimates {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                format!(
                    "swatch qualification '{}' has evidence=estimate without explicit allow_estimates=true",
                    self.id
                ),
            )
            .repair("measure a physical washed swatch or pass explicit estimate opt-in for dry-run prototyping"));
        }
        if !self.was_washed_and_blocked {
            return Err(Refusal::new(
                RefusalCode::UnqualifiedProcess,
                format!(
                    "swatch qualification '{}' was not washed and blocked before measuring gauge",
                    self.id
                ),
            )
            .repair("wash, dry flat/block the calibration swatch, and re-measure relaxed gauge"));
        }
        for (k, v) in [
            ("wales_per_100mm", self.wales_per_100mm),
            ("courses_per_100mm", self.courses_per_100mm),
            ("thickness_mm", self.thickness_mm),
            ("cam_stitch_setting", self.cam_stitch_setting),
        ] {
            Refusal::finite(k, v)?;
        }
        if self.wales_per_100mm <= 0.0 || self.courses_per_100mm <= 0.0 || self.thickness_mm <= 0.0
        {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "swatch gauge counts and thickness must be strictly positive",
            ));
        }
        if let Some(w) = self.wick_dispersion_mm {
            Refusal::finite("wick_dispersion_mm", w)?;
            if w < 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "wick_dispersion_mm cannot be negative",
                ));
            }
        }
        if let Some(s) = self.shrinkage_axial_pct {
            Refusal::finite("shrinkage_axial_pct", s)?;
        }
        if let Some(eps) = self.yarn_intake_drift_eps {
            Refusal::finite("yarn_intake_drift_eps", eps)?;
        }
        if let Some(sig) = self.yarn_intake_drift_sigma_eta {
            Refusal::finite("yarn_intake_drift_sigma_eta", sig)?;
            if sig < 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "yarn_intake_drift_sigma_eta cannot be negative",
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

#[derive(Serialize)]
struct DrapeReceiptSealView<'a> {
    schema: &'a str,
    pattern_sha256: &'a str,
    material_sha256: &'a str,
    max_strain_ratio: f64,
    jamming_margin: f64,
    edge_curl_rad: f64,
    crease_risk: f64,
    gating_flags: &'a [String],
    evidence: Evidence,
}

/// Sealed drape uncertainty & gating receipt handed across Seam 4 (`ftx-drape -> ftx-export`).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DrapeUncertaintyReceipt {
    /// Must equal [`DRAPE_UNCERTAINTY_SCHEMA`].
    pub schema: String,
    /// Digest of the draped pattern/knit IR.
    pub pattern_sha256: String,
    /// Digest of the material/constitutive card.
    pub material_sha256: String,
    /// Peak principal stretch ratio observed on the draped garment.
    pub max_strain_ratio: f64,
    /// Minimum margin to the yarn-jamming boundary (`<= 0` means jammed/flagged).
    pub jamming_margin: f64,
    /// Peak selvedge curl angle in radians.
    pub edge_curl_rad: f64,
    /// Dimensionless crease/buckling risk indicator in `[0, 1]`.
    pub crease_risk: f64,
    /// Non-empty gating flags (`KnitJamming`, `KnitEdgeCurl`, `KnitRibSnap`, `KnitPoreGrinThrough`, `KnitHysteresisDrift`, etc.).
    pub gating_flags: Vec<String>,
    /// Evidence basis of the constitutive model used during draping.
    pub evidence: Evidence,
    /// Sealed canonical SHA-256 over all fields above.
    pub payload_sha256: String,
}

impl DrapeUncertaintyReceipt {
    /// Seals a new [`DrapeUncertaintyReceipt`].
    ///
    /// # Errors
    /// [`RefusalCode::NonFinite`] if any scalar metric is non-finite.
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        pattern_sha256: String,
        material_sha256: String,
        max_strain_ratio: f64,
        jamming_margin: f64,
        edge_curl_rad: f64,
        crease_risk: f64,
        gating_flags: Vec<String>,
        evidence: Evidence,
    ) -> Result<Self, Refusal> {
        let view = DrapeReceiptSealView {
            schema: DRAPE_UNCERTAINTY_SCHEMA,
            pattern_sha256: &pattern_sha256,
            material_sha256: &material_sha256,
            max_strain_ratio,
            jamming_margin,
            edge_curl_rad,
            crease_risk,
            gating_flags: &gating_flags,
            evidence,
        };
        let payload_sha256 = canonical_json_sha256(&view)?;
        Ok(Self {
            schema: DRAPE_UNCERTAINTY_SCHEMA.to_string(),
            pattern_sha256,
            material_sha256,
            max_strain_ratio,
            jamming_margin,
            edge_curl_rad,
            crease_risk,
            gating_flags,
            evidence,
            payload_sha256,
        })
    }

    /// Recomputes and verifies [`Self::payload_sha256`].
    ///
    /// # Errors
    /// [`RefusalCode::VerificationMismatch`] if tampered or schema mismatches.
    pub fn verify_seal(&self) -> Result<(), Refusal> {
        if self.schema != DRAPE_UNCERTAINTY_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("unexpected drape receipt schema '{}'", self.schema),
            ));
        }
        let view = DrapeReceiptSealView {
            schema: &self.schema,
            pattern_sha256: &self.pattern_sha256,
            material_sha256: &self.material_sha256,
            max_strain_ratio: self.max_strain_ratio,
            jamming_margin: self.jamming_margin,
            edge_curl_rad: self.edge_curl_rad,
            crease_risk: self.crease_risk,
            gating_flags: &self.gating_flags,
            evidence: self.evidence,
        };
        let expected = canonical_json_sha256(&view)?;
        if expected != self.payload_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "drape uncertainty receipt payload_sha256 mismatch (tampered or unsealed)",
            ));
        }
        Ok(())
    }
}

/// Explicit Four-Claims separation carried on every knitting/dyeing certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FourClaimsVerdict {
    /// Reference parity basis (e.g. `"knitout_v2_vm_verified"` or `"none"`).
    pub parity_basis: String,
    /// Physics verification basis (e.g. `"poincloux_kat_and_rve_estimate"` or `"discrete_vm_only"`).
    pub physics_basis: String,
    /// Real-body fit claim: MUST ALWAYS be `false` for computed artifacts (only muslin/dressed photo establishes fit).
    pub fit_claimed: bool,
    /// Performance benchmark basis (`"unbenchmarked"` unless backed by measured hardware run).
    pub performance_basis: String,
}

#[derive(Serialize)]
struct KnitCertSealView<'a> {
    schema: &'a str,
    program_sha256: &'a str,
    machine_sha256: &'a str,
    qualification_sha256: &'a Option<String>,
    drape_receipt_sha256: &'a Option<String>,
    wearer_age_class: WearerAgeClass,
    hardware_execution_authorized: bool,
    physical_knitting_validated: bool,
    automated_carriage_control: bool,
    checks_passed: &'a [String],
    limitations: &'a [String],
    four_claims: &'a FourClaimsVerdict,
}

/// Sealed `ftx-machine-cert/1` machine verification certificate.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitMachineCert {
    /// Must equal [`KNIT_CERT_SCHEMA`] (`"ftx-machine-cert/1"`).
    pub schema: String,
    /// SHA-256 digest of the emitted Knitout / machine program.
    pub program_sha256: String,
    /// SHA-256 digest of the target [`super::profile::MachineProfile`].
    pub machine_sha256: String,
    /// SHA-256 digest of the [`KnitSwatchQualification`] (none only for `CalibrationSwatchOnly` or hand-knit).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub qualification_sha256: Option<String>,
    /// Optional SHA-256 digest of the sealed [`DrapeUncertaintyReceipt`].
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub drape_receipt_sha256: Option<String>,
    /// Required wearer age classification.
    pub wearer_age_class: WearerAgeClass,
    /// ALWAYS `false`: software verification never authorizes unattended mechanical execution.
    pub hardware_execution_authorized: bool,
    /// ALWAYS `false`: software verification never proves physical knitability without a machine test.
    pub physical_knitting_validated: bool,
    /// Derived strictly from [`super::profile::MachineProfile::automated_carriage_control`].
    pub automated_carriage_control: bool,
    /// Named discrete and physical verification checks that passed.
    pub checks_passed: Vec<String>,
    /// Explicit limitations and operator responsibilities.
    pub limitations: Vec<String>,
    /// Four-Claims separation record (`fit_claimed` always `false`).
    pub four_claims: FourClaimsVerdict,
    /// Sealed canonical SHA-256 over all fields above.
    pub cert_sha256: String,
}

impl KnitMachineCert {
    /// Seals a new [`KnitMachineCert`], unconditionally forcing `hardware_execution_authorized = false`,
    /// `physical_knitting_validated = false`, and `four_claims.fit_claimed = false`.
    ///
    /// # Errors
    /// [`RefusalCode::InvalidDocument`] if serialization fails.
    #[allow(clippy::too_many_arguments)]
    pub fn seal(
        program_sha256: String,
        machine_sha256: String,
        qualification_sha256: Option<String>,
        drape_receipt_sha256: Option<String>,
        wearer_age_class: WearerAgeClass,
        automated_carriage_control: bool,
        checks_passed: Vec<String>,
        mut limitations: Vec<String>,
        mut four_claims: FourClaimsVerdict,
    ) -> Result<Self, Refusal> {
        four_claims.fit_claimed = false;
        let mandatory_notice = "Software verification checks discrete V-bed kinematics and declared limits only; operator inspection, dry-run, and physical swatch validation are required before unattended machine execution.".to_string();
        if !limitations.contains(&mandatory_notice) {
            limitations.push(mandatory_notice);
        }
        let view = KnitCertSealView {
            schema: KNIT_CERT_SCHEMA,
            program_sha256: &program_sha256,
            machine_sha256: &machine_sha256,
            qualification_sha256: &qualification_sha256,
            drape_receipt_sha256: &drape_receipt_sha256,
            wearer_age_class,
            hardware_execution_authorized: false,
            physical_knitting_validated: false,
            automated_carriage_control,
            checks_passed: &checks_passed,
            limitations: &limitations,
            four_claims: &four_claims,
        };
        let cert_sha256 = canonical_json_sha256(&view)?;
        Ok(Self {
            schema: KNIT_CERT_SCHEMA.to_string(),
            program_sha256,
            machine_sha256,
            qualification_sha256,
            drape_receipt_sha256,
            wearer_age_class,
            hardware_execution_authorized: false,
            physical_knitting_validated: false,
            automated_carriage_control,
            checks_passed,
            limitations,
            four_claims,
            cert_sha256,
        })
    }

    /// Verifies seal integrity and constitutional false-booleans.
    ///
    /// # Errors
    /// [`RefusalCode::VerificationMismatch`] if tampered or if safety booleans were flipped to `true`.
    pub fn verify_integrity(&self) -> Result<(), Refusal> {
        if self.schema != KNIT_CERT_SCHEMA {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!("unexpected knit cert schema '{}'", self.schema),
            ));
        }
        if self.hardware_execution_authorized
            || self.physical_knitting_validated
            || self.four_claims.fit_claimed
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "knit machine certificate violates constitutional no-claim invariants (hardware_execution_authorized, physical_knitting_validated, and fit_claimed must be false)",
            ));
        }
        let view = KnitCertSealView {
            schema: &self.schema,
            program_sha256: &self.program_sha256,
            machine_sha256: &self.machine_sha256,
            qualification_sha256: &self.qualification_sha256,
            drape_receipt_sha256: &self.drape_receipt_sha256,
            wearer_age_class: self.wearer_age_class,
            hardware_execution_authorized: self.hardware_execution_authorized,
            physical_knitting_validated: self.physical_knitting_validated,
            automated_carriage_control: self.automated_carriage_control,
            checks_passed: &self.checks_passed,
            limitations: &self.limitations,
            four_claims: &self.four_claims,
        };
        let expected = canonical_json_sha256(&view)?;
        if expected != self.cert_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "knit machine certificate cert_sha256 mismatch (tampered)",
            ));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::ir::{
        Bed, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA, KNITOUT_SCHEMA,
        KnitDoc, KnitoutHeader, KnitoutOp, KnitoutProgram, LoopKind, Needle, ParentRef, StitchNode,
    };
    use crate::knit::profile::MachineProfile;

    #[test]
    fn knitout_serialization_preserves_headers_carriers_quarter_pitches_and_pause() {
        let prog = KnitoutProgram {
            schema: KNITOUT_SCHEMA.to_string(),
            header: KnitoutHeader {
                machine: "Kniterate".to_string(),
                gauge: "7".to_string(),
                width: 252,
                position: "Center".to_string(),
                carriers: vec![CarrierId("1".to_string()), CarrierId("2".to_string())],
                extra: BTreeMap::new(),
            },
            ops: vec![
                KnitoutOp::In {
                    carriers: vec![CarrierId("1".to_string())],
                },
                KnitoutOp::CarrierSpacing { needles: 2 },
                KnitoutOp::Rack {
                    quarter_pitches: -2,
                },
                KnitoutOp::Knit {
                    dir: CourseDir::Plus,
                    needle: Needle::new(Bed::Front, 10),
                    carriers: vec![CarrierId("1".to_string()), CarrierId("2".to_string())],
                },
                KnitoutOp::Amiss {
                    needle: Needle::new(Bed::Back, 10),
                },
                KnitoutOp::Pause {
                    note: Some("check yarn tension".to_string()),
                },
                KnitoutOp::Out {
                    carriers: vec![CarrierId("1".to_string())],
                },
            ],
        };
        let text = prog.to_knitout_text().unwrap();
        assert!(text.starts_with(";!knitout-2\n;;Machine: Kniterate\n;;Gauge: 7\n;;Width: 252\n"));
        assert!(text.contains("rack -0.5\n"));
        assert!(text.contains("knit + f10 1 2\n"));
        assert!(text.contains("amiss b10\n"));
        assert!(text.contains("pause ; check yarn tension\n"));
    }

    #[test]
    fn knit_doc_validates_dag_and_rejects_slider_knit() {
        let mut doc = KnitDoc {
            schema: KNIT_SCHEMA.to_string(),
            name: "swatch".to_string(),
            wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
            gauge: GaugeSpec {
                wales_per_100mm: 20.0,
                courses_per_100mm: 28.0,
                needle_pitch_mm: 25.4 / 7.0,
                yarn_tex: Some(180.0),
                thickness_mm: Some(2.1),
                relaxed_after_wash: true,
            },
            stitches: vec![
                StitchNode {
                    id: 0,
                    course: 0,
                    wale: 0,
                    bed: Bed::Front,
                    kind: LoopKind::CastOn,
                    parents: vec![],
                    carrier_ids: vec![CarrierId("1".to_string())],
                    yarn_id: "test-yarn".to_string(),
                    cam_stitch_setting: CamStitchSetting {
                        setting: 8.0,
                        machine_family: "kniterate_7e".to_string(),
                    },
                    yarn_length_mm: Some(9.5),
                },
                StitchNode {
                    id: 1,
                    course: 1,
                    wale: 0,
                    bed: Bed::Front,
                    kind: LoopKind::Knit,
                    parents: vec![ParentRef {
                        stitch_id: 0,
                        stack_pos: 0,
                    }],
                    carrier_ids: vec![CarrierId("1".to_string())],
                    yarn_id: "test-yarn".to_string(),
                    cam_stitch_setting: CamStitchSetting {
                        setting: 8.0,
                        machine_family: "kniterate_7e".to_string(),
                    },
                    yarn_length_mm: Some(9.5),
                },
            ],
            short_row_turns: vec![],
            passes: vec![],
            cable_groups: vec![],
            yarn_runs: vec![],
            has_hood_or_neck_drawstring: false,
            metadata: BTreeMap::new(),
        };
        doc.validate().unwrap();
        let sha_before = doc.canonical_sha256().unwrap();
        let mut json_val = serde_json::to_value(&doc).unwrap();
        let roundtripped: KnitDoc = serde_json::from_value(json_val.clone()).unwrap();
        assert_eq!(roundtripped, doc);
        assert_eq!(roundtripped.canonical_sha256().unwrap(), sha_before);

        // Unknown field rejected by deny_unknown_fields on KnitDoc.
        let mut with_unknown = json_val.clone();
        with_unknown
            .as_object_mut()
            .unwrap()
            .insert("phantom_field".to_string(), serde_json::json!(1));
        assert!(serde_json::from_value::<KnitDoc>(with_unknown).is_err());

        // Missing non-defaulting wearer_age_class fails closed.
        json_val.as_object_mut().unwrap().remove("wearer_age_class");
        assert!(serde_json::from_value::<KnitDoc>(json_val).is_err());

        // Mutating doc changes canonical SHA-256.
        let mut tampered = doc.clone();
        tampered.name = "tampered".to_string();
        assert_ne!(tampered.canonical_sha256().unwrap(), sha_before);

        doc.stitches[1].bed = Bed::FrontSlider;
        let err = doc.validate().unwrap_err();
        assert_eq!(err.code, RefusalCode::MachineStateInfeasible);
    }

    #[test]
    fn sealed_receipts_detect_tampering_and_enforce_honesty_booleans() {
        let prof = MachineProfile::kniterate_7e();
        let mhash = prof.canonical_sha256().unwrap();
        let receipt = DrapeUncertaintyReceipt::seal(
            "pat".to_string(),
            "mat".to_string(),
            1.15,
            0.25,
            0.12,
            0.05,
            vec![],
            Evidence::Estimate,
        )
        .unwrap();
        receipt.verify_seal().unwrap();
        let mut tampered = receipt.clone();
        tampered.jamming_margin = 0.99;
        assert_eq!(
            tampered.verify_seal().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );

        let mut cert = KnitMachineCert::seal(
            "prog".to_string(),
            mhash,
            None,
            Some(receipt.payload_sha256.clone()),
            WearerAgeClass::CalibrationSwatchOnly,
            prof.automated_carriage_control,
            vec!["vm_ok".to_string()],
            vec![],
            FourClaimsVerdict {
                parity_basis: "knitout_v2".to_string(),
                physics_basis: "discrete_vm_only".to_string(),
                fit_claimed: true, // seal must force false!
                performance_basis: "unbenchmarked".to_string(),
            },
        )
        .unwrap();
        assert!(!cert.hardware_execution_authorized);
        assert!(!cert.physical_knitting_validated);
        assert!(!cert.four_claims.fit_claimed);
        cert.verify_integrity().unwrap();
        cert.hardware_execution_authorized = true;
        assert_eq!(
            cert.verify_integrity().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
    }

    #[test]
    fn swatch_qualification_rejects_unwashed_and_unknown_fields() {
        let json = r#"{
            "id": "sw-1",
            "material_sha256": "m1",
            "machine_sha256": "k1",
            "yarn_lot_id": "lot-42",
            "was_washed_and_blocked": false,
            "wales_per_100mm": 22.0,
            "courses_per_100mm": 30.0,
            "thickness_mm": 2.0,
            "cam_stitch_setting": 7.5,
            "evidence": "measured",
            "source": {"reference": "local:sw1", "accessed_on": "2026-11-27", "note": "swatch"},
            "method": "flat dry 24h"
        }"#;
        let q: KnitSwatchQualification = serde_json::from_str(json).unwrap();
        assert_eq!(
            q.validate_for_production("m1", "k1").unwrap_err().code,
            RefusalCode::UnqualifiedProcess
        );
        // Unknown field rejected by deny_unknown_fields
        let bad_json = json.replace("\"lot-42\",", "\"lot-42\", \"phantom\": 1,");
        assert!(serde_json::from_str::<KnitSwatchQualification>(&bad_json).is_err());

        let mut nan_shrink = q;
        nan_shrink.was_washed_and_blocked = true;
        nan_shrink.shrinkage_axial_pct = Some(f64::NAN);
        assert_eq!(
            nan_shrink
                .validate_for_production("m1", "k1")
                .unwrap_err()
                .code,
            RefusalCode::NonFinite
        );
    }

    #[test]
    fn machine_profile_and_knitout_reject_nan_and_control_injection() {
        let mut prof = MachineProfile::kniterate_7e();
        prof.cam_setting_range = [f64::NAN, 15.0];
        assert_eq!(prof.validate().unwrap_err().code, RefusalCode::NonFinite);

        let bad_prog = KnitoutProgram {
            schema: KNITOUT_SCHEMA.to_string(),
            header: KnitoutHeader {
                machine: "Kniterate\ninject".to_string(),
                gauge: "7".to_string(),
                width: 252,
                position: "Center".to_string(),
                carriers: vec![CarrierId("1".to_string())],
                extra: BTreeMap::new(),
            },
            ops: vec![],
        };
        assert_eq!(
            bad_prog.to_knitout_text().unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        let empty_in = KnitoutProgram {
            schema: KNITOUT_SCHEMA.to_string(),
            header: KnitoutHeader {
                machine: "Kniterate".to_string(),
                gauge: "7".to_string(),
                width: 252,
                position: "Center".to_string(),
                carriers: vec![CarrierId("1".to_string())],
                extra: BTreeMap::new(),
            },
            ops: vec![KnitoutOp::In { carriers: vec![] }],
        };
        assert_eq!(
            empty_in.to_knitout_text().unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }
}
