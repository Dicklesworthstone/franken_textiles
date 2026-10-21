//! First-order span uncertainty along correlated lens modes. Refit the *same* inlier
//! correspondences for every lens perturbation: lens and measurement homography are dependent.
use ftx_core::{Refusal, RefusalCode, det};
use serde_json::json;

use super::{
    Calibration, PhotoScale, Samples,
    geometry::{Homography, Point, fit, plane_scale, plane_scale_in_image, undistort_point},
    intrinsics::IntrinsicUncertainty,
    invalid,
    measurement::on_plane,
};
use crate::{bundle::cholesky, camera::Intrinsics};

fn factor(u: &IntrinsicUncertainty) -> Result<Vec<Vec<f64>>, Refusal> {
    if u.schema != "ftx-intrinsic-uncertainty/1"
        || u.parameters != ["fx", "fy", "cx", "cy", "k1", "k2", "k3", "p1", "p2"]
    {
        return Err(invalid(
            "unsupported intrinsic covariance schema/parameter order",
        ));
    }
    u.corner_noise.validate()?;
    u.intrinsics.validate()?;
    let sigma: Vec<_> = (0..9).map(|i| u.covariance[i][i].sqrt()).collect();
    if sigma.iter().enumerate().any(|(i, s)| {
        !s.is_finite()
            || *s <= 0.0
            || !u.standard_deviations[i].is_finite()
            || (u.standard_deviations[i] - s).abs() > 1e-10 * s
    }) {
        return Err(invalid(
            "intrinsic covariance needs consistent positive finite diagonal sigmas",
        ));
    }
    let mut correlation = vec![vec![0.; 9]; 9];
    for i in 0..9 {
        for j in 0..9 {
            let a = u.covariance[i][j];
            let b = u.covariance[j][i];
            if !a.is_finite() || !b.is_finite() || a != b {
                return Err(invalid("intrinsic covariance must be finite and symmetric"));
            }
            correlation[i][j] = a / sigma[i] / sigma[j];
        }
    }
    let mut l = cholesky(&correlation, 0.0, 1e-12)
        .ok_or_else(|| invalid("intrinsic covariance is not numerically positive definite"))?;
    for (row, s) in l.iter_mut().zip(sigma) {
        for v in row {
            *v *= s;
        }
    }
    Ok(l)
}

impl PhotoScale {
    /// Attach a first-order Brown-lens covariance from an independent calibration acquisition.
    /// Requires this exact lens and a usable ChArUco scale. Does not invent a noise model or
    /// combine calibration and photo evidence from the same acquisition. Caller evidence is an
    /// assertion, not verified physical independence. Repeated measurements share lens error.
    pub fn with_lens_uncertainty(
        mut self,
        uncertainty: &IntrinsicUncertainty,
        independence_evidence: &str,
    ) -> Result<Self, Refusal> {
        if independence_evidence.trim().is_empty() || independence_evidence.len() > 4096 {
            return Err(invalid(
                "independent calibration evidence is required for additive lens uncertainty",
            ));
        }
        factor(uncertainty)?;
        let c = self.calibration.as_mut().ok_or_else(|| {
            Refusal::new(
                RefusalCode::Unscaled,
                "a rejected scale cannot acquire usable lens uncertainty",
            )
        })?;
        let k = c
            .intrinsics
            .ok_or_else(|| invalid("lens uncertainty requires calibrated ChArUco coordinates"))?;
        if !matches!(c.samples, Samples::Charuco { .. })
            || json!(k) != json!(uncertainty.intrinsics)
        {
            return Err(invalid(
                "intrinsic covariance must belong to the exact photo lens",
            ));
        }
        c.intrinsic_uncertainty = Some(uncertainty.clone());
        self.report["intrinsic_uncertainty"] = json!({
            "calibration":uncertainty, "independence_evidence":independence_evidence,
            "propagation":"first-order correlated lens modes; refit fixed inlier correspondences; two central-difference step sizes",
            "no_claim":"No joint calibration/photo noise correlation, print-size/flatness error or lens drift. Measurements using this lens share calibration error."});
        for note in self.report["notes"].as_array_mut().into_iter().flatten() {
            if note
                .as_str()
                .is_some_and(|s| s.contains("Intrinsics are assumed exact"))
            {
                *note = json!(
                    "Brown lens correction and conditional intrinsic covariance apply to corners and clicks. The measurement homography is refitted as the lens varies; calibration noise and acquisition independence are explicit assumptions."
                );
            }
        }
        Ok(self)
    }
}

struct Map {
    k: Intrinsics,
    h: Homography,
    centre: Point,
}
struct Mode {
    step: f64,
    // +h, -h, +h/2, -h/2; directions already include covariance correlations and physical units.
    maps: [Map; 4],
}
pub(super) struct Modes(Vec<Mode>);

fn shift(mut k: Intrinsics, delta: [f64; 9]) -> Result<Intrinsics, Refusal> {
    k.focal_px.x += delta[0];
    k.focal_px.y += delta[1];
    k.principal_px.x += delta[2];
    k.principal_px.y += delta[3];
    for (v, d) in k.distortion.radial.iter_mut().zip(&delta[4..7]) {
        *v += d;
    }
    for (v, d) in k.distortion.tangential.iter_mut().zip(&delta[7..9]) {
        *v += d;
    }
    k.validate()?;
    Ok(k)
}

impl Modes {
    pub(super) fn prepare(c: &Calibration) -> Result<Option<Self>, Refusal> {
        let Some(u) = &c.intrinsic_uncertainty else {
            return Ok(None);
        };
        let l = factor(u)?;
        let Samples::Charuco {
            observed, board, ..
        } = &c.samples
        else {
            return Err(invalid(
                "intrinsic covariance requires identified ChArUco inliers",
            ));
        };
        let k = c.intrinsics.unwrap();
        let scales = [
            k.focal_px.x,
            k.focal_px.y,
            c.image[0],
            c.image[1],
            1.,
            1.,
            1.,
            1.,
            1.,
        ];
        let mut modes = Vec::new();
        let directions: [[f64; 9]; 9] = std::array::from_fn(|j| std::array::from_fn(|i| l[i][j]));
        for direction in directions {
            let magnitude = direction
                .iter()
                .zip(scales)
                .map(|(d, s)| d.abs() / s)
                .fold(0., f64::max);
            let step = 1e-5 / magnitude;
            if !step.is_finite() || step <= 0.0 {
                return Err(invalid(
                    "intrinsic covariance perturbation is not representable",
                ));
            }
            let mut maps = Vec::new();
            for t in [step, -step, step * 0.5, -step * 0.5] {
                let k = shift(k, direction.map(|d| d * t))?;
                let pixels = observed
                    .iter()
                    .map(|p| undistort_point(*p, Some(&k)))
                    .collect::<Result<Vec<_>, _>>()?;
                let h = fit(&pixels, board)?;
                plane_scale_in_image(&h, c.size, Some(&k))?;
                let centre = plane_scale(&h, c.size)?.centre;
                maps.push(Map { k, h, centre });
            }
            modes.push(Mode {
                step,
                maps: maps.try_into().ok().unwrap(),
            });
        }
        Ok(Some(Self(modes)))
    }

    pub(super) fn sigma(
        &self,
        p: Point,
        q: Point,
        delta: Point,
        angle: bool,
    ) -> Result<f64, Refusal> {
        let distance = det::hypot(delta[0], delta[1]);
        // Differentiate the signed angle before folding to 0..90 degrees, as click uncertainty
        // does. Central differences of abs(angle) would falsely give zero sigma at horizontal.
        let u = if angle {
            [-delta[1] / distance, delta[0] / distance]
        } else {
            [delta[0] / distance, delta[1] / distance]
        };
        let units = if angle {
            (1.0 / distance).to_degrees()
        } else {
            1.0
        };
        let mut variance = 0.;
        for mode in &self.0 {
            let mut spans = Vec::new();
            for m in &mode.maps {
                let a = on_plane(&m.h, undistort_point(p, Some(&m.k))?, m.centre)?;
                let b = on_plane(&m.h, undistort_point(q, Some(&m.k))?, m.centre)?;
                spans.push([b[0] - a[0], b[1] - a[1]]);
            }
            let derivative = |a: Point, b: Point, step| {
                units * (u[0] * (a[0] - b[0]) + u[1] * (a[1] - b[1])) / step
            };
            let coarse = derivative(spans[0], spans[1], 2.0 * mode.step);
            let fine = derivative(spans[2], spans[3], mode.step);
            if !coarse.is_finite()
                || !fine.is_finite()
                || (coarse - fine).abs() > 0.01 * fine.abs() + 1e-6 * distance * units
            {
                return Err(Refusal::new(
                    RefusalCode::NotConverged,
                    "lens uncertainty linearization is unstable at these landmarks",
                ));
            }
            variance += fine * fine;
        }
        Refusal::finite("intrinsic calibration sigma", variance.sqrt())
    }
}
