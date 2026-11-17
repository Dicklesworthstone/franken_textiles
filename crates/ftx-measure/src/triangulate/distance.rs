//! A straight distance between two source-bound triangulated landmarks.
//!
//! Both endpoints retain the same complete camera/capture cohort. Shared camera modes act on
//! both points together; their scalar responses are subtracted before squaring. Endpoint clicks
//! are independent only under an explicit declaration. No component is total uncertainty.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use serde::{Deserialize, Serialize};

use super::{
    TriangulationInput, TriangulationReport,
    uncertainty::{
        ClickNoiseInput, ClickUncertaintyReport, LensNoiseInput, LensUncertaintyReport,
        ReferenceScaleNoiseInput, SheetCornerNoiseInput, SheetUncertaintyReport,
        triangulate_with_click_noise, triangulate_with_lens_noise,
        triangulate_with_reference_scale_noise, triangulate_with_sheet_noise,
        validate_camera_noise_sources, validate_reference_scale_sources,
    },
};
use crate::invalid;

/// One named endpoint with its original image observations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoint {
    /// Distinct nonempty operator label; no anatomical meaning is inferred.
    pub name: String,
    /// Complete original triangulation input, including its FIT and CHECK observations.
    pub input: TriangulationInput,
    /// Optional independent click model. Both endpoints must declare one together.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_noise: Option<ClickNoiseInput>,
}

/// Exactly two endpoints in one source-bound metric camera and capture cohort.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    /// Must be ftx-triangulation-distance-input/1.
    pub schema: String,
    /// Ordered endpoint labels and original observations; never an unbounded point batch.
    pub endpoints: [Endpoint; 2],
    /// Basis for independence between both endpoints' clicks, required with click models.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub click_independence_evidence: Option<String>,
    /// One original correlated calibration acquisition shared by both endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lens_noise: Option<LensNoiseInput>,
    /// One complete set of original sheet-corner acquisitions shared by both endpoints.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sheet_noise: Option<SheetCornerNoiseInput>,
    /// One common uniform source scale, about the original sheet coordinate origin.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_scale_noise: Option<ReferenceScaleNoiseInput>,
}

/// Original endpoint geometry, with all residuals and gates retained.
#[derive(Clone, Debug, Serialize)]
pub struct EndpointReport {
    /// Unchanged operator label.
    pub name: String,
    /// Ordinary triangulation; a distance request never changes its estimator or gates.
    pub triangulation: TriangulationReport,
}

/// One separately conditional first-order contribution to straight-distance precision.
#[derive(Clone, Debug, Serialize)]
pub struct ConditionalUncertainty {
    /// AVAILABLE or UNAVAILABLE; exact shared-mode cancellation may legitimately give zero.
    pub status: &'static str,
    /// Scalar variance in square metres; null when this complete component is unavailable.
    pub variance_m2: Option<f64>,
    /// One-sigma scalar contribution in metres, not a simultaneous confidence bound.
    pub sigma_m: Option<f64>,
    /// Numerical or original-geometry failures; no partial variance is published.
    pub flags: Vec<String>,
    /// The assumptions and limitations of this component.
    pub no_claim: &'static str,
}

/// A geometric straight distance, with independently requested precision components.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// ftx-triangulation-distance/1.
    pub schema: &'static str,
    /// ACCEPTED or REJECTED geometry. Requested precision can remain UNAVAILABLE independently.
    pub status: &'static str,
    /// Complete parsed request, preserving the operator's endpoint order and original evidence.
    pub input: Input,
    /// SHA-256 of canonical serialized parsed input, not a digest of original file bytes.
    pub input_sha256: String,
    /// Both unchanged endpoint geometry reports, including any refusal of a usable point.
    pub endpoints: [EndpointReport; 2],
    /// Positive finite straight distance in metres, absent when geometry is not admitted.
    pub distance_m: Option<f64>,
    /// Independent endpoint-click contribution, only when requested for both endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_click_uncertainty: Option<ConditionalUncertainty>,
    /// Correlated shared-lens contribution, retaining the declared pose model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_lens_uncertainty: Option<ConditionalUncertainty>,
    /// Shared sheet-corner contribution through both original camera-pose estimators.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_sheet_uncertainty: Option<ConditionalUncertainty>,
    /// Common reference-scale contribution; never averaged across views or endpoints.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub conditional_reference_scale_uncertainty: Option<ConditionalUncertainty>,
    /// Reasons no usable straight distance was published.
    pub flags: Vec<String>,
    /// Scope of this geometric measurement, separate from tape definitions and readiness.
    pub no_claim: &'static str,
}

const CLICK_SCOPE: &str = "First-order independent endpoint-click precision with fixed cameras, scale and capture. Independence is explicitly declared, not established. Excludes camera, scale, correspondence, motion and systematic errors; not total uncertainty.";
const LENS_SCOPE: &str = "First-order shared-lens precision, with each original correlated mode applied to both endpoints under its declared fixed-pose or source-pose model. Other precision components stay separate; not total uncertainty.";
const SHEET_SCOPE: &str = "First-order shared sheet-corner precision: each original FIT corner perturbs both endpoint camera estimators together. Lens and metric coordinates stay fixed; not total uncertainty.";
const SCALE_SCOPE: &str = "One source-bound common uniform reference-scale mode. Distance sigma equals distance times declared fractional sigma, independent of coordinate origin and view count. Excludes nonuniform print distortion, warp, uncertain origin and all other errors; not total uncertainty.";

fn bytes(value: &impl Serialize) -> Result<Vec<u8>, Refusal> {
    serde_json::to_vec(value).map_err(|error| invalid(error.to_string()))
}

fn numerical(message: &str) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
}

fn validate_pair(input: &Input) -> Result<(), Refusal> {
    if input.schema != "ftx-triangulation-distance-input/1"
        || input
            .endpoints
            .iter()
            .any(|endpoint| !super::text_present(&endpoint.name))
        || input.endpoints[0].name == input.endpoints[1].name
    {
        return Err(invalid(
            "distance needs schema /1 and two distinct nonempty endpoint names",
        ));
    }
    let [a, b] = &input.endpoints;
    if a.input.metric_scale_source != b.input.metric_scale_source
        || bytes(&a.input.capture)? != bytes(&b.input.capture)?
        || a.input.observations.len() != b.input.observations.len()
        || a.input.observations.len() > 128
    {
        return Err(invalid(
            "distance endpoints must retain the same metric source, capture and complete frame cohort",
        ));
    }
    let mut left: Vec<_> = a.input.observations.iter().collect();
    let mut right: Vec<_> = b.input.observations.iter().collect();
    left.sort_by(|a, b| a.frame.cmp(&b.frame));
    right.sort_by(|a, b| a.frame.cmp(&b.frame));
    for (a, b) in left.iter().zip(&right) {
        if a.frame != b.frame
            || a.partition != b.partition
            || bytes(&a.camera)? != bytes(&b.camera)?
        {
            return Err(invalid(
                "both distance endpoints must use the same camera and FIT/CHECK role in every frame",
            ));
        }
    }
    match (
        &a.click_noise,
        &b.click_noise,
        &input.click_independence_evidence,
    ) {
        (None, None, None) => {}
        (Some(_), Some(_), Some(evidence)) if super::text_present(evidence) => {}
        _ => {
            return Err(invalid(
                "distance click precision needs both complete endpoint noise models and explicit cross-endpoint independence evidence",
            ));
        }
    }
    if let (Some(lens), Some(sheet)) = (&input.lens_noise, &input.sheet_noise) {
        validate_camera_noise_sources(lens, sheet)?;
    }
    if let Some(scale) = &input.reference_scale_noise {
        validate_reference_scale_sources(
            scale,
            input.lens_noise.as_ref(),
            input.sheet_noise.as_ref(),
        )?;
    }
    Ok(())
}

fn separation(a: Vec3, b: Vec3) -> Result<(f64, Vec3), Refusal> {
    let delta = b - a;
    let scale = delta.x.abs().max(delta.y.abs()).max(delta.z.abs());
    if !delta.is_finite() || !scale.is_finite() || scale == 0. {
        return Err(numerical(
            "endpoint separation is zero or unrepresentable; no distance derivative exists",
        ));
    }
    let scaled = delta / scale;
    let norm = scaled.norm();
    let distance = scale * norm;
    if !distance.is_finite() || distance <= 0. {
        return Err(numerical("straight distance is not representable"));
    }
    Ok((distance, scaled / norm))
}

#[derive(Default)]
struct Variance(f64);

impl Variance {
    fn mode(&mut self, response_m: f64) -> Result<(), Refusal> {
        let square = response_m * response_m;
        if !response_m.is_finite() || !square.is_finite() || (response_m != 0. && square == 0.) {
            return Err(numerical(
                "distance variance contribution overflowed or underflowed",
            ));
        }
        self.0 += square;
        if !self.0.is_finite() {
            return Err(numerical("distance variance sum overflowed"));
        }
        Ok(())
    }

    fn direction(&mut self, mode: Vec3, unit: Vec3, sigma: f64) -> Result<(), Refusal> {
        if !mode.is_finite() || !sigma.is_finite() || sigma <= 0. {
            return Err(numerical("non-finite distance sensitivity"));
        }
        let mut products = [0.; 3];
        for axis in 0..3 {
            products[axis] = mode[axis] * unit[axis];
            if mode[axis] != 0. && unit[axis] != 0. && products[axis] == 0. {
                return Err(numerical("distance projection underflowed"));
            }
        }
        let response = products.iter().sum::<f64>();
        let weighted = response * sigma;
        if !response.is_finite() || !weighted.is_finite() || (response != 0. && weighted == 0.) {
            return Err(numerical(
                "projected distance sensitivity overflowed or underflowed",
            ));
        }
        self.mode(weighted)
    }
}

fn component(
    direction: Option<Vec3>,
    statuses: [&str; 2],
    flags: [&[String]; 2],
    scope: &'static str,
    evaluate: impl FnOnce(Vec3) -> Result<f64, Refusal>,
) -> ConditionalUncertainty {
    let mut report = ConditionalUncertainty {
        status: "UNAVAILABLE",
        variance_m2: None,
        sigma_m: None,
        flags: Vec::new(),
        no_claim: scope,
    };
    for index in 0..2 {
        if statuses[index] != "AVAILABLE" {
            report.flags.push(format!(
                "Endpoint {} precision unavailable: {}",
                index + 1,
                flags[index].join("; "),
            ));
        }
    }
    let Some(direction) = direction else {
        report
            .flags
            .push("No accepted nonzero endpoint separation".into());
        return report;
    };
    if !report.flags.is_empty() {
        return report;
    }
    match evaluate(direction) {
        Ok(variance) => {
            report.status = "AVAILABLE";
            report.variance_m2 = Some(variance);
            report.sigma_m = Some(variance.sqrt());
        }
        Err(error) => report.flags.push(error.to_string()),
    }
    report
}

fn click_variance(
    a: &ClickUncertaintyReport,
    b: &ClickUncertaintyReport,
    unit: Vec3,
) -> Result<f64, Refusal> {
    let mut components = [0.; 2];
    for (index, report) in [a, b].into_iter().enumerate() {
        if report.sensitivities.len() != report.input.observations.len() {
            return Err(numerical("incomplete endpoint click sensitivities"));
        }
        let mut variance = Variance::default();
        for frame in &report.sensitivities {
            for axis in 0..2 {
                variance.direction(
                    Vec3::new(
                        frame.jacobian_m_per_px[0][axis],
                        frame.jacobian_m_per_px[1][axis],
                        frame.jacobian_m_per_px[2][axis],
                    ),
                    unit,
                    frame.sigma_px,
                )?;
            }
        }
        components[index] = variance.0;
    }
    // Separate endpoint sums preserve exact swap symmetry without changing the declared
    // independence of their individual click coordinates.
    let total = components[0] + components[1];
    if !total.is_finite() {
        return Err(numerical("distance click variance sum overflowed"));
    }
    Ok(total)
}

fn lens_variance(
    a: &LensUncertaintyReport,
    b: &LensUncertaintyReport,
    unit: Vec3,
) -> Result<f64, Refusal> {
    let right: BTreeMap<_, _> = b
        .sensitivities
        .iter()
        .map(|group| (group.key.as_str(), group))
        .collect();
    if a.input_sha256 != b.input_sha256
        || a.sensitivities.len() != b.sensitivities.len()
        || right.len() != b.sensitivities.len()
    {
        return Err(numerical("unmatched shared lens acquisition modes"));
    }
    let mut variance = Variance::default();
    for left in &a.sensitivities {
        let other = right
            .get(left.key.as_str())
            .ok_or_else(|| numerical("missing shared lens group"))?;
        if left.lens_id != other.lens_id
            || left.calibration_sha256 != other.calibration_sha256
            || left.frames != other.frames
        {
            return Err(numerical("shared lens mode source identities differ"));
        }
        for (a, b) in left
            .mode_sensitivities_m
            .iter()
            .zip(&other.mode_sensitivities_m)
        {
            variance.direction(*b - *a, unit, 1.)?;
        }
    }
    Ok(variance.0)
}

fn sheet_variance(
    a: &SheetUncertaintyReport,
    b: &SheetUncertaintyReport,
    unit: Vec3,
) -> Result<f64, Refusal> {
    let right: BTreeMap<_, _> = b
        .sensitivities
        .iter()
        .map(|frame| (&frame.frame, frame))
        .collect();
    if a.input_sha256 != b.input_sha256
        || a.sensitivities.len() != b.sensitivities.len()
        || right.len() != b.sensitivities.len()
    {
        return Err(numerical("unmatched shared sheet acquisition modes"));
    }
    let mut variance = Variance::default();
    for left in &a.sensitivities {
        let other = right
            .get(&left.frame)
            .ok_or_else(|| numerical("missing shared sheet frame"))?;
        if left.sheet_pose_sha256 != other.sheet_pose_sha256
            || left.fit_point_indices != other.fit_point_indices
            || left.sigma_px != other.sigma_px
            || left.jacobians_m_per_px.len() != left.fit_point_indices.len()
            || other.jacobians_m_per_px.len() != other.fit_point_indices.len()
        {
            return Err(numerical("shared sheet corner source identities differ"));
        }
        for (a, b) in left
            .jacobians_m_per_px
            .iter()
            .zip(&other.jacobians_m_per_px)
        {
            for axis in 0..2 {
                variance.direction(
                    Vec3::new(
                        b[0][axis] - a[0][axis],
                        b[1][axis] - a[1][axis],
                        b[2][axis] - a[2][axis],
                    ),
                    unit,
                    left.sigma_px,
                )?;
            }
        }
    }
    Ok(variance.0)
}

/// Reconstruct both original endpoints and their source-bound straight-distance precision.
///
/// Exactly two original estimators run per requested component, each retaining its existing
/// observation, source and differentiation budgets. The complete request is limited to 16 MiB.
/// Shared camera modes are differenced before squaring; conditional components are never summed.
///
/// # Errors
/// Invalid/incomplete evidence, mismatched cameras/capture/scale, unknown fields on deserialization,
/// and oversized requests refuse. Rejected geometry and numerical precision failures instead retain
/// diagnostic reports with no usable distance or no affected conditional variance, respectively.
pub fn run(input: &Input) -> Result<Report, Refusal> {
    let input_bytes = bytes(input)?;
    if input_bytes.len() > 16 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "complete distance input exceeds 16 MiB",
        ));
    }
    validate_pair(input)?;
    let a = super::triangulate(&input.endpoints[0].input)?;
    let b = super::triangulate(&input.endpoints[1].input)?;
    let mut flags = Vec::new();
    let geometry = match (a.point_m, b.point_m) {
        (Some(a), Some(b)) => match separation(a, b) {
            Ok(geometry) => Some(geometry),
            Err(error) => {
                flags.push(error.to_string());
                None
            }
        },
        _ => {
            flags.push("Both endpoints must pass ordinary triangulation gates".into());
            None
        }
    };
    let mut report = Report {
        schema: "ftx-triangulation-distance/1",
        status: if geometry.is_some() {
            "ACCEPTED"
        } else {
            "REJECTED"
        },
        input: input.clone(),
        input_sha256: sha256_hex(&input_bytes),
        endpoints: [
            EndpointReport {
                name: input.endpoints[0].name.clone(),
                triangulation: a,
            },
            EndpointReport {
                name: input.endpoints[1].name.clone(),
                triangulation: b,
            },
        ],
        distance_m: geometry.map(|(distance, _)| distance),
        conditional_click_uncertainty: None,
        conditional_lens_uncertainty: None,
        conditional_sheet_uncertainty: None,
        conditional_reference_scale_uncertainty: None,
        flags,
        no_claim: "Straight Euclidean separation of two explicitly identified stationary or synchronized landmarks, conditional on the complete original metric camera cohort and capture declarations. Not a surface path, tape girth, anatomical correspondence, fused measurement, readiness upgrade, total uncertainty, real-camera accuracy or body/garment fit.",
    };
    let direction = geometry.map(|(_, unit)| unit);
    let a = &input.endpoints[0];
    let b = &input.endpoints[1];
    if let (Some(left), Some(right)) = (&a.click_noise, &b.click_noise) {
        let left = triangulate_with_click_noise(&a.input, left)?.conditional_click_uncertainty;
        let right = triangulate_with_click_noise(&b.input, right)?.conditional_click_uncertainty;
        report.conditional_click_uncertainty = Some(component(
            direction,
            [left.status, right.status],
            [&left.flags, &right.flags],
            CLICK_SCOPE,
            |unit| click_variance(&left, &right, unit),
        ));
    }
    if let Some(noise) = &input.lens_noise {
        let left = triangulate_with_lens_noise(&a.input, noise)?.conditional_lens_uncertainty;
        let right = triangulate_with_lens_noise(&b.input, noise)?.conditional_lens_uncertainty;
        report.conditional_lens_uncertainty = Some(component(
            direction,
            [left.status, right.status],
            [&left.flags, &right.flags],
            LENS_SCOPE,
            |unit| lens_variance(&left, &right, unit),
        ));
    }
    if let Some(noise) = &input.sheet_noise {
        let left = triangulate_with_sheet_noise(&a.input, noise)?.conditional_sheet_uncertainty;
        let right = triangulate_with_sheet_noise(&b.input, noise)?.conditional_sheet_uncertainty;
        report.conditional_sheet_uncertainty = Some(component(
            direction,
            [left.status, right.status],
            [&left.flags, &right.flags],
            SHEET_SCOPE,
            |unit| sheet_variance(&left, &right, unit),
        ));
    }
    if let Some(noise) = &input.reference_scale_noise {
        let left = triangulate_with_reference_scale_noise(&a.input, noise)?
            .conditional_reference_scale_uncertainty;
        let right = triangulate_with_reference_scale_noise(&b.input, noise)?
            .conditional_reference_scale_uncertainty;
        // Replaying both sources is required, but positional covariance is not the scalar
        // authority: large common coordinates can cancel to a representable distance.
        report.conditional_reference_scale_uncertainty = Some(component(
            direction,
            ["AVAILABLE"; 2],
            [&[], &[]],
            SCALE_SCOPE,
            |_| {
                if left.input_sha256 != right.input_sha256
                    || left.reference_origin_m != right.reference_origin_m
                {
                    return Err(numerical("shared reference-scale sources differ"));
                }
                let distance = report
                    .distance_m
                    .ok_or_else(|| numerical("no accepted straight distance"))?;
                let response = distance * noise.sigma_fraction;
                if response == 0. {
                    return Err(numerical("distance scale sensitivity underflowed"));
                }
                let mut variance = Variance::default();
                variance.mode(response)?;
                Ok(variance.0)
            },
        ));
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        camera::{Camera, Distortion, Intrinsics},
        triangulate::{
            Capture, FrameId, Observation, Partition,
            uncertainty::{FrameNoise, LensGroupSensitivity, LensNoiseGroup},
        },
    };
    use ftx_core::{Mat3, Vec2};
    use serde_json::json;

    fn fixture() -> Input {
        let endpoint = |name: &str, point| {
            let observations: Vec<_> = [-0.5, 0.5, 0.]
                .into_iter()
                .enumerate()
                .map(|(index, x)| {
                    let camera = Camera {
                        intrinsics: Intrinsics {
                            focal_px: Vec2::new(800., 800.),
                            principal_px: Vec2::new(800., 600.),
                            distortion: Distortion::default(),
                        },
                        world_to_camera: Mat3::from_rows([
                            [1., 0., 0.],
                            [0., 0., -1.],
                            [0., 1., 0.],
                        ]),
                        center_m: Vec3::new(x, -2., 1.),
                    };
                    Observation {
                        frame: FrameId {
                            stream: format!("synthetic-distance-{index}"),
                            timestamp_us: 9_007_199_254_740_993,
                        },
                        camera,
                        pixel_px: camera.project(point).unwrap(),
                        partition: if index == 2 {
                            Partition::HeldOut
                        } else {
                            Partition::Fit
                        },
                    }
                })
                .collect();
            let noise = ClickNoiseInput {
                schema: "ftx-click-noise/1".into(),
                evidence: "Synthetic independent one-pixel endpoint clicks".into(),
                observations: observations[..2]
                    .iter()
                    .map(|view| FrameNoise {
                        frame: view.frame.clone(),
                        sigma_px: 1.,
                    })
                    .collect(),
            };
            Endpoint {
                name: name.into(),
                input: TriangulationInput {
                    schema: "ftx-triangulation-input/1".into(),
                    metric_scale_source: "Synthetic one-metre common baseline".into(),
                    capture: Capture::Synchronized {
                        max_skew_us: 0,
                        evidence: "Synthetic simultaneous stationary endpoints".into(),
                    },
                    observations,
                },
                click_noise: Some(noise),
            }
        };
        Input {
            schema: "ftx-triangulation-distance-input/1".into(),
            endpoints: [
                endpoint("left", Vec3::new(-0.1, 0., 1.)),
                endpoint("right", Vec3::new(0.1, 0., 1.)),
            ],
            click_independence_evidence: Some(
                "Synthetic independent clicks between endpoints and images".into(),
            ),
            lens_noise: None,
            sheet_noise: None,
            reference_scale_noise: None,
        }
    }

    #[test]
    fn stereo_distance_and_independent_click_precision_match_closed_form() {
        let input = fixture();
        let report = run(&input).unwrap();
        assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
        assert!((report.distance_m.unwrap() - 0.2).abs() < 1e-12);
        for (endpoint, original) in report.endpoints.iter().zip(&input.endpoints) {
            assert_eq!(
                json!(endpoint.triangulation),
                json!(super::super::triangulate(&original.input).unwrap()),
            );
            assert_eq!(
                endpoint.triangulation.input.observations[0]
                    .frame
                    .timestamp_us,
                9_007_199_254_740_993,
            );
        }
        // x = b * (u_left + u_right) / (2 * (u_left - u_right)).
        // At x = +/-0.1, z = 2, b = 1, f = 800 the two endpoint x derivatives
        // are [0.0015, 0.001] and [0.001, 0.0015] metres per pixel.
        let precision = report.conditional_click_uncertainty.unwrap();
        assert_eq!(precision.status, "AVAILABLE", "{:?}", precision.flags);
        assert!((precision.variance_m2.unwrap() - 6.5e-6).abs() < 1e-12);
        assert!((precision.sigma_m.unwrap() - 0.0025495097567963926).abs() < 1e-10);
        assert_eq!(report.input_sha256, sha256_hex(&bytes(&input).unwrap()));
        assert!(report.conditional_lens_uncertainty.is_none());

        let mut reversed = input;
        reversed.endpoints.swap(0, 1);
        for endpoint in &mut reversed.endpoints {
            endpoint.input.observations.reverse();
            endpoint
                .click_noise
                .as_mut()
                .unwrap()
                .observations
                .reverse();
        }
        let reversed = run(&reversed).unwrap();
        assert_eq!(reversed.distance_m, report.distance_m);
        assert_eq!(
            reversed.conditional_click_uncertainty.unwrap().variance_m2,
            precision.variance_m2,
        );
    }

    #[test]
    fn original_gates_and_unavailable_precision_never_become_zero_error() {
        let mut input = fixture();
        for endpoint in &mut input.endpoints {
            for noise in &mut endpoint.click_noise.as_mut().unwrap().observations {
                noise.sigma_px = 1e-300;
            }
        }
        let report = run(&input).unwrap();
        assert_eq!(report.status, "ACCEPTED");
        assert!(report.distance_m.is_some());
        let precision = report.conditional_click_uncertainty.unwrap();
        assert_eq!(precision.status, "UNAVAILABLE");
        assert!(precision.variance_m2.is_none());
        assert!(precision.sigma_m.is_none());

        let mut input = fixture();
        input.endpoints[1].input.observations[2].pixel_px.x += 50.;
        let report = run(&input).unwrap();
        assert_eq!(report.status, "REJECTED");
        assert!(report.distance_m.is_none());
        assert!(report.endpoints[0].triangulation.point_m.is_some());
        assert!(report.endpoints[1].triangulation.point_m.is_none());
        assert_eq!(
            report.conditional_click_uncertainty.unwrap().status,
            "UNAVAILABLE",
        );

        let mut input = fixture();
        input.endpoints[1].input = input.endpoints[0].input.clone();
        let report = run(&input).unwrap();
        assert_eq!(report.status, "REJECTED");
        assert!(
            report
                .endpoints
                .iter()
                .all(|r| r.triangulation.point_m.is_some())
        );
        assert!(report.distance_m.is_none());
        assert!(
            report
                .conditional_click_uncertainty
                .unwrap()
                .sigma_m
                .is_none()
        );
    }

    #[test]
    fn endpoints_require_identical_sources_cameras_frames_capture_and_roles() {
        let input = fixture();
        for variant in 0..7 {
            let mut changed = input.clone();
            let endpoint = &mut changed.endpoints[1];
            match variant {
                0 => endpoint.input.metric_scale_source.push_str(" changed"),
                1 => {
                    if let Capture::Synchronized { evidence, .. } = &mut endpoint.input.capture {
                        evidence.push_str(" changed");
                    }
                }
                2 => endpoint.input.observations[2].camera.intrinsics.focal_px.x += 1.,
                3 => endpoint.input.observations[2].partition = Partition::Fit,
                4 => endpoint.input.observations[0].frame.timestamp_us += 1,
                5 => endpoint.name = "left".into(),
                _ => {
                    endpoint.input.observations.pop();
                }
            }
            assert!(run(&changed).is_err(), "variant {variant}");
        }
        for variant in 0..5 {
            let mut changed = input.clone();
            match variant {
                0 => changed.endpoints[1].click_noise = None,
                1 => changed.click_independence_evidence = None,
                2 => changed.click_independence_evidence = Some(" ".into()),
                3 => {
                    changed.endpoints[0]
                        .click_noise
                        .as_mut()
                        .unwrap()
                        .observations
                        .pop();
                }
                _ => changed.schema.push_str("-unknown"),
            }
            assert!(run(&changed).is_err(), "noise variant {variant}");
        }
        let mut no_noise = input;
        no_noise.click_independence_evidence = None;
        for endpoint in &mut no_noise.endpoints {
            endpoint.click_noise = None;
        }
        assert!(
            run(&no_noise)
                .unwrap()
                .conditional_click_uncertainty
                .is_none()
        );
        let mut unknown = json!(no_noise);
        unknown["total_sigma_m"] = json!(0.);
        assert!(serde_json::from_value::<Input>(unknown).is_err());
    }

    fn mode_report(modes: [Vec3; 9]) -> LensUncertaintyReport {
        // Reducer-only source identity. Acquisition admission is exercised through the real
        // calibrated-camera integration test; no fabricated calibration enters run().
        LensUncertaintyReport {
            schema: "ftx-triangulation-lens-uncertainty/1",
            status: "AVAILABLE",
            input: LensNoiseInput {
                schema: "ftx-triangulation-lens-noise/1".into(),
                independence_evidence: "Synthetic scalar mode algebra".into(),
                groups: vec![LensNoiseGroup {
                    key: "shared".into(),
                    calibration: json!({"reducer_test_only":true}),
                    frames: Vec::new(),
                }],
                sheet_poses: None,
            },
            input_sha256: "synthetic-common-mode-source".into(),
            covariance_m2: None,
            sigma_m: None,
            sensitivities: vec![LensGroupSensitivity {
                key: "shared".into(),
                lens_id: "synthetic-mode-law".into(),
                frames: Vec::new(),
                calibration_sha256: "synthetic-common-calibration".into(),
                mode_sensitivities_m: modes,
                covariance_m2: Mat3::from_rows([[0.; 3]; 3]),
            }],
            flags: Vec::new(),
            no_claim: "Synthetic reducer arithmetic only",
        }
    }

    #[test]
    fn shared_translation_and_rotation_cancel_before_variance_is_formed() {
        let mut left = [Vec3::ZERO; 9];
        let mut right = left;
        // Common principal-point translation: dx/dcx = -depth/f = -0.0025 m/px.
        left[0] = Vec3::new(-0.0025, 0., 0.);
        right[0] = left[0];
        // A common rigid rotation about z changes this x-directed segment only in y.
        left[1] = Vec3::new(0., -0.1, 0.);
        right[1] = Vec3::new(0., 0.1, 0.);
        let unit = Vec3::new(1., 0., 0.);
        assert_eq!(
            lens_variance(&mode_report(left), &mode_report(right), unit).unwrap(),
            0.,
        );
        left[2] = Vec3::new(-0.001, 0., 0.);
        right[2] = Vec3::new(0.001, 0., 0.);
        assert_eq!(
            lens_variance(&mode_report(left), &mode_report(right), unit).unwrap(),
            4e-6,
        );
        let mut changed = mode_report(right);
        changed.sensitivities[0]
            .calibration_sha256
            .push_str("-changed");
        assert!(lens_variance(&mode_report(left), &changed, unit).is_err());
    }

    #[test]
    fn scaled_separation_and_variance_arithmetic_do_not_invent_perfect_precision() {
        for scale in [1e-200, 1e200] {
            let (distance, direction) =
                separation(Vec3::ZERO, Vec3::new(scale, scale, 0.)).unwrap();
            assert!((distance / scale - 2_f64.sqrt()).abs() < 1e-15);
            assert!((direction.norm() - 1.).abs() < 1e-15);
        }
        for response in [1e-300, 1e300, f64::NAN, f64::INFINITY] {
            assert!(Variance::default().mode(response).is_err());
        }
        let mut variance = Variance::default();
        variance
            .direction(Vec3::new(0., 1., 0.), Vec3::new(1., 0., 0.), 1.)
            .unwrap();
        assert_eq!(variance.0, 0.);
        assert!(
            Variance::default()
                .direction(Vec3::new(1e-300, 0., 0.), Vec3::new(1e-300, 1., 0.), 1.)
                .is_err(),
        );
    }
}
