//! Body-interface sizing for the existing three-branch pullover drafter.
//!
//! Only four equivalent girths come from the body. Underarm lengths and yoke depth remain
//! explicit garment design choices: a shoulder-to-wrist tape is not an underarm sleeve length.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode};
use serde::{Deserialize, Serialize};

use crate::BodyMeasure;

use super::{
    BranchedPulloverSpec, CarrierId, GaugeSpec, KnitDoc, MachineProfile, RaglanPulloverSpec,
    WearerAgeClass, draft_branched_raglan_pullover,
};

/// The only body measurements consumed by this garment program, in canonical catalogue names.
pub const PULLOVER_BODY_MEASUREMENTS: [&str; 4] = ["chest", "neck_base", "upper_arm", "wrist"];

/// Portable/native design file; source measurement and accepted-body documents stay separate.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyPulloverInput {
    /// Explicit built-in knitting machine identifier, resolved by the consumer.
    pub machine: String,
    /// Stated relaxed gauge; production export still requires its ordinary qualification.
    pub gauge: GaugeSpec,
    /// Explicit garment design and signed wearing ease.
    pub design: BodyPulloverSpec,
    /// Whether the ordinary measurement fusion may use automatic chart priors.
    pub use_priors: bool,
}

/// Body-relative girths and independently authored garment dimensions; no dimension defaults.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BodyPulloverSpec {
    /// Garment name.
    pub name: String,
    /// Explicit wearer classification; calibration-swatch exemptions are forbidden.
    pub wearer_age_class: WearerAgeClass,
    /// Signed finished chest circumference minus body chest, in mm.
    pub chest_ease_mm: f64,
    /// Signed finished neck circumference minus body neck-base girth, in mm.
    pub neck_ease_mm: f64,
    /// Signed finished upper-arm circumference minus body upper arm, in mm.
    pub upper_arm_ease_mm: f64,
    /// Signed finished cuff circumference minus body wrist girth, in mm.
    pub wrist_ease_mm: f64,
    /// Authored underarm-to-neck yoke height, excluding additional collar rib, in mm.
    pub yoke_depth_mm: f64,
    /// Authored underarm-to-hem length, including hem rib, in mm.
    pub body_length_below_arm_mm: f64,
    /// Authored underarm sleeve length, excluding additional cuff rib, in mm.
    pub sleeve_length_below_arm_mm: f64,
    /// Hem rib courses inside the stated body length.
    pub hem_rib_courses: u32,
    /// Collar rib courses added above the stated yoke.
    pub collar_rib_courses: u32,
    /// Cuff rib courses added below the stated plain sleeve length.
    pub cuff_rib_courses: u32,
    /// Explicit cam setting for the selected yarn and machine.
    pub cam_setting: f64,
    /// Yarn identifier, subject to the normal qualification binding.
    pub yarn_id: String,
    /// Yarn supply for the body branch.
    pub body_carrier_id: CarrierId,
    /// Separate supply for the left sleeve, continued through the yoke.
    pub left_sleeve_carrier_id: CarrierId,
    /// Separate supply for the right sleeve.
    pub right_sleeve_carrier_id: CarrierId,
    /// Existing child drawstring rules apply unchanged.
    pub has_hood_or_neck_drawstring: bool,
}

impl BodyPulloverSpec {
    /// Check the explicit safety class against supplied age, without inferring an absent age.
    /// Ages three through fourteen use child rules; an infant is younger than 36 months.
    ///
    /// # Errors
    /// Invalid age or an explicit classification inconsistent with the source wearer.
    pub fn check_wearer_age(&self, age_months: Option<f64>) -> Result<(), Refusal> {
        let Some(age) = age_months else {
            return Ok(());
        };
        Refusal::finite("wearer.age_months", age)?;
        if age < 0.0 {
            return Err(invalid("wearer age cannot be negative"));
        }
        let expected = if age < 36.0 {
            WearerAgeClass::InfantUnder3
        } else if age < 180.0 {
            WearerAgeClass::Child3To14
        } else {
            WearerAgeClass::Adult
        };
        if self.wearer_age_class != expected {
            return Err(Refusal::new(
                RefusalCode::MaterialProcessHazard,
                "explicit wearer_age_class disagrees with the supplied source age",
            ));
        }
        Ok(())
    }
}

/// Source class selected by the caller that owns the body interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum BodySizingBasis {
    /// Recomputed measurement evidence; individual source provenance remains in the source file.
    FusedMeasurements,
    /// Predictions from the explicitly accepted model shape, never new measurements.
    ModelFit,
    /// Authored garment dimensions, not inferred body measurements.
    GarmentDesign,
}

/// One exact requested dimension and the dimension actually formed at the selected gauge.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BodyDimension {
    /// Measurement/model/design basis; does not establish physical fit.
    pub basis: BodySizingBasis,
    /// Canonical body measurement name, absent for authored lengths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub measurement: Option<String>,
    /// Consumed body value before ease, absent for authored lengths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub body_value_mm: Option<f64>,
    /// Body interface uncertainty when supplied; a model's absent uncertainty stays absent.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sigma_mm: Option<f64>,
    /// Explicit signed circumference ease, absent for authored lengths.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ease_mm: Option<f64>,
    /// Requested finished dimension before integer stitch/course quantization.
    pub requested_mm: f64,
    /// Actual stitch/course count times relaxed gauge, without display rounding.
    pub actual_mm: f64,
    /// Actual minus requested, without display rounding.
    pub residual_mm: f64,
}

/// Sizing and design decisions retained beside the ordinary knitting document.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BodyPulloverReport {
    /// Exact finished-dimension input passed to the existing branched drafter.
    pub resolved_spec: BranchedPulloverSpec,
    /// Source values, explicit ease and actual gauge residuals by finished dimension.
    pub dimensions: BTreeMap<String, BodyDimension>,
    /// Actual cuff-to-underarm length including the separately requested cuff rib.
    pub sleeve_total_length_mm: f64,
    /// Actual neck-to-underarm height including the separately requested collar rib.
    pub yoke_with_collar_length_mm: f64,
    /// Limits that remain relevant when using body-derived garment targets.
    pub notes: Vec<String>,
}

/// A complete three-branch stitch graph and its separate sizing explanation.
#[derive(Clone, Debug)]
pub struct BodyPulloverDraft {
    /// Existing `ftx-knit/1` document; ordinary compilation/qualification still applies.
    pub doc: KnitDoc,
    /// Exact body/design/quantization decisions.
    pub report: BodyPulloverReport,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

/// Draft a complete pullover from equivalent body girths and explicit design lengths.
/// The body implementation owns where values came from; this function never fits, substitutes
/// a chart, derives an underarm length from an arm tape, or certifies opening passage/stretch.
///
/// # Errors
/// Missing/nonfinite body values, invalid uncertainty/ease, swatch exemptions, impossible finished
/// dimensions, or any refusal from the existing branched pullover drafter.
pub fn draft_pullover_from_body(
    body: &dyn BodyMeasure,
    basis: BodySizingBasis,
    design: &BodyPulloverSpec,
    gauge: &GaugeSpec,
    profile: &MachineProfile,
) -> Result<BodyPulloverDraft, Refusal> {
    if basis == BodySizingBasis::GarmentDesign {
        return Err(invalid("body girths require a measurement or model basis"));
    }
    if design.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "a measured pullover is wearable and cannot use calibration-swatch exemptions",
        ));
    }
    let mut dimensions = BTreeMap::new();
    for (target, measurement, ease) in [
        ("chest_girth_mm", "chest", design.chest_ease_mm),
        ("neck_girth_mm", "neck_base", design.neck_ease_mm),
        (
            "sleeve_upper_arm_girth_mm",
            "upper_arm",
            design.upper_arm_ease_mm,
        ),
        ("sleeve_cuff_girth_mm", "wrist", design.wrist_ease_mm),
    ] {
        let value = body.value(measurement).map_err(|_| {
            invalid(format!(
                "measured pullover requires body measurement '{measurement}'"
            ))
        })?;
        Refusal::finite(measurement, value)?;
        Refusal::finite(&format!("{measurement} ease"), ease)?;
        let requested = value + ease;
        Refusal::finite(target, requested)?;
        if value <= 0.0 || requested <= 0.0 {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("{measurement} and its finished girth after signed ease must be positive"),
            ));
        }
        let sigma = body.sigma(measurement);
        if sigma.is_some_and(|value| !value.is_finite() || value < 0.0) {
            return Err(invalid(format!(
                "invalid body uncertainty for '{measurement}'"
            )));
        }
        dimensions.insert(
            target.into(),
            BodyDimension {
                basis,
                measurement: Some(measurement.into()),
                body_value_mm: Some(value),
                sigma_mm: sigma,
                ease_mm: Some(ease),
                requested_mm: requested,
                actual_mm: 0.0,
                residual_mm: 0.0,
            },
        );
    }
    let spec = BranchedPulloverSpec {
        torso: RaglanPulloverSpec {
            name: design.name.clone(),
            wearer_age_class: design.wearer_age_class,
            neck_girth_mm: dimensions["neck_girth_mm"].requested_mm,
            chest_girth_mm: dimensions["chest_girth_mm"].requested_mm,
            yoke_depth_mm: design.yoke_depth_mm,
            body_length_below_arm_mm: design.body_length_below_arm_mm,
            collar_rib_courses: design.collar_rib_courses,
            hem_rib_courses: design.hem_rib_courses,
            cam_setting: design.cam_setting,
            yarn_id: design.yarn_id.clone(),
            carrier_id: design.body_carrier_id.clone(),
            has_hood_or_neck_drawstring: design.has_hood_or_neck_drawstring,
        },
        sleeve_cuff_girth_mm: dimensions["sleeve_cuff_girth_mm"].requested_mm,
        sleeve_upper_arm_girth_mm: dimensions["sleeve_upper_arm_girth_mm"].requested_mm,
        sleeve_length_mm: design.sleeve_length_below_arm_mm,
        cuff_rib_courses: design.cuff_rib_courses,
        left_sleeve_carrier: design.left_sleeve_carrier_id.clone(),
        right_sleeve_carrier: design.right_sleeve_carrier_id.clone(),
    };
    let mut doc = draft_branched_raglan_pullover(&spec, gauge, profile)?;
    let count = |name: &str| -> Result<u32, Refusal> {
        doc.metadata
            .get(name)
            .and_then(|value| value.parse().ok())
            .ok_or_else(|| invalid(format!("branched draft omitted actual count '{name}'")))
    };
    for (name, count_key) in [
        ("neck_girth_mm", "neck_stitch_count"),
        ("chest_girth_mm", "chest_stitch_count"),
        ("sleeve_upper_arm_girth_mm", "upper_arm_stitch_count"),
        ("sleeve_cuff_girth_mm", "cuff_stitch_count"),
    ] {
        let dimension = dimensions.get_mut(name).expect("all four girths inserted");
        dimension.actual_mm = f64::from(count(count_key)?) * gauge.blocked_wale_pitch_mm();
        dimension.residual_mm = dimension.actual_mm - dimension.requested_mm;
    }
    for (name, requested, count_key) in [
        ("yoke_depth_mm", design.yoke_depth_mm, "yoke_course_count"),
        (
            "body_length_below_arm_mm",
            design.body_length_below_arm_mm,
            "body_course_count",
        ),
        (
            "sleeve_length_below_arm_mm",
            design.sleeve_length_below_arm_mm,
            "sleeve_plain_course_count",
        ),
    ] {
        let actual = f64::from(count(count_key)?) * gauge.blocked_course_height_mm();
        dimensions.insert(
            name.into(),
            BodyDimension {
                basis: BodySizingBasis::GarmentDesign,
                measurement: None,
                body_value_mm: None,
                sigma_mm: None,
                ease_mm: None,
                requested_mm: requested,
                actual_mm: actual,
                residual_mm: actual - requested,
            },
        );
    }
    let report = BodyPulloverReport {
        sleeve_total_length_mm: f64::from(count("sleeve_plain_course_count")? + design.cuff_rib_courses)
            * gauge.blocked_course_height_mm(),
        yoke_with_collar_length_mm: f64::from(count("yoke_course_count")? + design.collar_rib_courses)
            * gauge.blocked_course_height_mm(),
        resolved_spec: spec, dimensions,
        notes: vec![
            "Body girths and garment design lengths remain separate. Model predictions and chart priors do not become measured evidence or establish readiness.".into(),
            "Sleeves are symmetric, using the stated upper-arm and wrist measurement definitions; individual left/right anatomy is not inferred.".into(),
            "Head/hand passage, stretch under load and seam-free physical knitting are not established by relaxed circumference or signed ease. Check the garment on a muslin or qualified physical knit.".into(),
        ],
    };
    doc.metadata.insert(
        "body_sizing".into(),
        serde_json::to_string(&report).map_err(|error| invalid(error.to_string()))?,
    );
    Ok(BodyPulloverDraft { doc, report })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Measurements;
    use serde_json::json;

    fn fixture() -> (Measurements, BodyPulloverSpec, GaugeSpec, MachineProfile) {
        let body =
            Measurements(json!({"chest":180.0,"neck_base":90.0,"upper_arm":60.0,"wrist":40.0}));
        let profile = MachineProfile::hand_knit(4.0);
        let design = BodyPulloverSpec {
            name: "synthetic-body-pullover".into(),
            wearer_age_class: WearerAgeClass::Adult,
            chest_ease_mm: 21.0,
            neck_ease_mm: 10.0,
            upper_arm_ease_mm: 21.0,
            wrist_ease_mm: -1.0,
            yoke_depth_mm: 40.0,
            body_length_below_arm_mm: 80.0,
            sleeve_length_below_arm_mm: 60.0,
            hem_rib_courses: 2,
            collar_rib_courses: 2,
            cuff_rib_courses: 2,
            cam_setting: 13.0,
            yarn_id: "synthetic-yarn".into(),
            body_carrier_id: CarrierId("2".into()),
            left_sleeve_carrier_id: CarrierId("1".into()),
            right_sleeve_carrier_id: CarrierId("3".into()),
            has_hood_or_neck_drawstring: false,
        };
        let gauge = GaugeSpec {
            wales_per_100mm: 20.0,
            courses_per_100mm: 25.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        (body, design, gauge, profile)
    }

    #[test]
    fn body_girths_and_signed_ease_reach_the_real_branched_compiler() {
        let (body, design, gauge, profile) = fixture();
        let draft = draft_pullover_from_body(
            &body,
            BodySizingBasis::FusedMeasurements,
            &design,
            &gauge,
            &profile,
        )
        .unwrap();
        let chest = &draft.report.dimensions["chest_girth_mm"];
        assert_eq!(chest.body_value_mm, Some(180.0));
        assert_eq!(chest.requested_mm, 201.0);
        assert_eq!(chest.actual_mm, 200.0);
        assert_eq!(chest.residual_mm, -1.0);
        assert_eq!(
            draft.report.dimensions["sleeve_cuff_girth_mm"].ease_mm,
            Some(-1.0)
        );
        assert_eq!(
            draft.report.dimensions["sleeve_cuff_girth_mm"].residual_mm,
            1.0
        );
        assert_eq!(
            draft.report.dimensions["sleeve_length_below_arm_mm"].basis,
            BodySizingBasis::GarmentDesign
        );
        assert_eq!(draft.report.sleeve_total_length_mm, 68.0);
        assert_eq!(draft.report.yoke_with_collar_length_mm, 48.0);
        let compiled =
            super::super::compile_knit_doc(&draft.doc, &profile, None, None, None).unwrap();
        assert!(!compiled.cert.four_claims.fit_claimed);
    }

    #[test]
    fn model_predictions_keep_their_basis_and_missing_uncertainty() {
        let (body, design, gauge, profile) = fixture();
        let draft =
            draft_pullover_from_body(&body, BodySizingBasis::ModelFit, &design, &gauge, &profile)
                .unwrap();
        for measurement in draft
            .report
            .dimensions
            .values()
            .filter(|d| d.measurement.is_some())
        {
            assert_eq!(measurement.basis, BodySizingBasis::ModelFit);
            assert_eq!(measurement.sigma_mm, None);
        }
        assert_eq!(
            draft.report.dimensions["yoke_depth_mm"].basis,
            BodySizingBasis::GarmentDesign
        );
        assert!(draft.doc.metadata["body_sizing"].contains("MODEL_FIT"));
    }

    #[test]
    fn missing_body_values_invalid_ease_and_wearable_swatch_exemptions_refuse() {
        let (mut body, mut design, gauge, profile) = fixture();
        body.0.as_object_mut().unwrap().remove("wrist");
        assert!(
            draft_pullover_from_body(
                &body,
                BodySizingBasis::FusedMeasurements,
                &design,
                &gauge,
                &profile
            )
            .unwrap_err()
            .message
            .contains("wrist")
        );
        body.0["wrist"] = json!(40.0);
        design.wrist_ease_mm = -40.0;
        assert!(
            draft_pullover_from_body(
                &body,
                BodySizingBasis::FusedMeasurements,
                &design,
                &gauge,
                &profile
            )
            .is_err()
        );
        design.wrist_ease_mm = f64::NAN;
        assert_eq!(
            draft_pullover_from_body(
                &body,
                BodySizingBasis::FusedMeasurements,
                &design,
                &gauge,
                &profile
            )
            .unwrap_err()
            .code,
            RefusalCode::NonFinite
        );
        design.wrist_ease_mm = 0.0;
        design.wearer_age_class = WearerAgeClass::CalibrationSwatchOnly;
        assert_eq!(
            draft_pullover_from_body(
                &body,
                BodySizingBasis::FusedMeasurements,
                &design,
                &gauge,
                &profile
            )
            .unwrap_err()
            .code,
            RefusalCode::MaterialProcessHazard
        );
    }

    #[test]
    fn source_age_cannot_be_downgraded_to_an_adult_or_swatch_gate() {
        let (_, mut design, _, _) = fixture();
        assert!(design.check_wearer_age(Some(30.0)).is_err());
        design.wearer_age_class = WearerAgeClass::InfantUnder3;
        design.check_wearer_age(Some(30.0)).unwrap();
        assert!(design.check_wearer_age(Some(36.0)).is_err());
        design.wearer_age_class = WearerAgeClass::Child3To14;
        design.check_wearer_age(Some(36.0)).unwrap();
        design.check_wearer_age(Some(179.0)).unwrap();
        assert!(design.check_wearer_age(Some(180.0)).is_err());
    }
}
