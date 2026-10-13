//! Calibrated cameras for plan §10.2. World coordinates are metres, z up; camera coordinates
//! are x right, y down, z forward. Pixels must already have their EXIF rotation applied.
//! This module consumes calibration, not images. It does not establish calibration accuracy.

use ftx_core::{Mat3, Refusal, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::invalid;

/// Brown radial/tangential distortion in normalized camera coordinates (not pixels).
/// The declared disk is conservatively certified injective before inversion is allowed.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Distortion {
    /// Coefficients k1, k2, k3 of r², r⁴, r⁶.
    pub radial: [f64; 3],
    /// Coefficients p1, p2 of decentering distortion.
    pub tangential: [f64; 2],
    /// Maximum undistorted normalized radius; no extrapolation outside this disk.
    pub max_radius: f64,
}

impl Default for Distortion {
    fn default() -> Self {
        Self {
            radial: [0.0; 3],
            tangential: [0.0; 2],
            max_radius: 2.0,
        }
    }
}

impl Distortion {
    /// Reject non-finite or potentially folding calibrations rather than select an inverse branch.
    /// This sufficient bound is conservative: a rejected lens may work on a smaller declared disk.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.max_radius.is_finite()
            || self.max_radius <= 0.0
            || self.max_radius > 10.0
            || self
                .radial
                .iter()
                .chain(self.tangential.iter())
                .any(|x| !x.is_finite())
        {
            return Err(invalid("invalid lens coefficients or radius (expected 0 < radius <= 10)"));
        }
        // Radial Jacobian eigenvalues are R and R+2r²R'. Dropping positive terms bounds both
        // from below on the entire disk, not just at sampled radii. The tangential Jacobian
        // is symmetric and its spectral norm is at most 8*r*(|p1|+|p2|). Positive definiteness
        // throughout a convex disk implies strong monotonicity, hence a unique inverse there.
        let r2 = self.max_radius * self.max_radius;
        let mut power = r2;
        let mut lower = 1.0;
        for (i, k) in self.radial.iter().enumerate() {
            lower += (2 * i + 3) as f64 * k.min(0.0) * power;
            power *= r2;
        }
        lower -= 8.0 * self.max_radius * (self.tangential[0].abs() + self.tangential[1].abs());
        if !lower.is_finite() || lower <= 1e-6 {
            return Err(invalid(
                "lens inverse is not certified on its declared disk; reduce max_radius or recalibrate",
            ));
        }
        Ok(())
    }

    fn inside(&self, point: Vec2) -> bool {
        point.is_finite() && point.norm2() <= self.max_radius * self.max_radius
    }

    fn evaluate(&self, point: Vec2) -> (Vec2, [f64; 3]) {
        let Vec2 { x, y } = point;
        let r2 = x * x + y * y;
        let [k1, k2, k3] = self.radial;
        let [p1, p2] = self.tangential;
        let radial = 1.0 + r2 * (k1 + r2 * (k2 + r2 * k3));
        let slope = k1 + r2 * (2.0 * k2 + 3.0 * r2 * k3);
        let distorted = Vec2::new(
            x * radial + 2.0 * p1 * x * y + p2 * (r2 + 2.0 * x * x),
            y * radial + p1 * (r2 + 2.0 * y * y) + 2.0 * p2 * x * y,
        );
        let jacobian = [
            radial + 2.0 * x * x * slope + 2.0 * p1 * y + 6.0 * p2 * x,
            2.0 * x * y * slope + 2.0 * p1 * x + 2.0 * p2 * y,
            radial + 2.0 * y * y * slope + 6.0 * p1 * y + 2.0 * p2 * x,
        ];
        (distorted, jacobian)
    }

    /// Apply distortion to an undistorted normalized point inside the calibrated disk.
    pub fn distort(&self, point: Vec2) -> Result<Vec2, Refusal> {
        self.validate()?;
        if !self.inside(point) {
            return Err(invalid("point is outside the calibrated lens disk"));
        }
        let (out, _) = self.evaluate(point);
        if !out.is_finite() {
            return Err(invalid("non-finite lens projection"));
        }
        Ok(out)
    }

    /// Invert distortion using bounded, residual-decreasing Newton steps. No silent iteration stop.
    pub fn undistort(&self, target: Vec2) -> Result<Vec2, Refusal> {
        self.validate()?;
        if !target.is_finite() || !target.norm2().is_finite() {
            return Err(invalid("distorted coordinates must be finite"));
        }
        let tolerance = 1e-13 * (1.0 + target.norm());
        let mut point = Vec2::ZERO;
        for _ in 0..60 {
            let (image, [a, b, c]) = self.evaluate(point);
            let residual = image - target;
            let error = residual.norm();
            if error <= tolerance {
                return Ok(point);
            }
            let determinant = a * c - b * b;
            if !determinant.is_finite() || determinant <= 0.0 {
                return Err(invalid("singular lens inverse"));
            }
            let step = Vec2::new(
                (c * residual.x - b * residual.y) / determinant,
                (a * residual.y - b * residual.x) / determinant,
            );
            let mut alpha = 1.0;
            let mut accepted = None;
            for _ in 0..32 {
                let trial = point - step * alpha;
                if self.inside(trial) && (self.evaluate(trial).0 - target).norm() < error {
                    accepted = Some(trial);
                    break;
                }
                alpha *= 0.5;
            }
            point = accepted.ok_or_else(|| {
                invalid("lens inverse stalled or lies outside calibration; no ray was published")
            })?;
        }
        Err(invalid("lens inverse did not converge; no ray was published"))
    }
}

/// Intrinsics for EXIF-oriented pixels. Skew and fisheye models are not supported.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Intrinsics {
    /// Positive focal lengths fx, fy in pixels.
    pub focal_px: Vec2,
    /// Principal point cx, cy in pixels.
    pub principal_px: Vec2,
    /// Brown distortion; omitted means an undistorted pinhole with normalized radius <= 2.
    #[serde(default)]
    pub distortion: Distortion,
}

impl Intrinsics {
    /// Validate numeric parameters and the distortion domain.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.focal_px.is_finite()
            || self.focal_px.x <= 0.0
            || self.focal_px.y <= 0.0
            || !self.principal_px.is_finite()
        {
            return Err(invalid("camera needs finite positive focal lengths and a finite principal point"));
        }
        self.distortion.validate()
    }

    /// Project a camera-space point; points on or behind the camera plane are refused.
    pub fn project(&self, point: Vec3) -> Result<Vec2, Refusal> {
        self.validate()?;
        if !point.is_finite() || point.z <= 0.0 {
            return Err(invalid("point must be finite and in front of the camera"));
        }
        let q = self.distortion.distort(Vec2::new(point.x / point.z, point.y / point.z))?;
        let pixel = Vec2::new(
            self.focal_px.x * q.x + self.principal_px.x,
            self.focal_px.y * q.y + self.principal_px.y,
        );
        if !pixel.is_finite() {
            return Err(invalid("non-finite pixel projection"));
        }
        Ok(pixel)
    }

    /// Recover a unit camera-space bearing from an oriented pixel.
    pub fn bearing(&self, pixel: Vec2) -> Result<Vec3, Refusal> {
        self.validate()?;
        let q = self.distortion.undistort(Vec2::new(
            (pixel.x - self.principal_px.x) / self.focal_px.x,
            (pixel.y - self.principal_px.y) / self.focal_px.y,
        ))?;
        Ok(Vec3::new(q.x, q.y, 1.0).normalized())
    }
}

/// Camera with a metric world-space center, not a world-to-camera translation vector.
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Camera {
    /// Lens calibration for the oriented image.
    pub intrinsics: Intrinsics,
    /// Right-handed orthonormal rotation: X_camera = R * (X_world - center_m).
    pub world_to_camera: Mat3,
    /// Optical center in metres, in the shared z-up world frame.
    pub center_m: Vec3,
}

impl Camera {
    /// Reject reflections, scaled rotations and non-finite camera poses.
    pub fn validate(&self) -> Result<(), Refusal> {
        self.intrinsics.validate()?;
        if !self.center_m.is_finite()
            || self.world_to_camera.rows.iter().flatten().any(|x| !x.is_finite())
        {
            return Err(invalid("camera pose must be finite"));
        }
        let gram = self.world_to_camera.transpose() * self.world_to_camera;
        for (i, row) in gram.rows.iter().enumerate() {
            for (j, value) in row.iter().enumerate() {
                let expected = if i == j { 1.0 } else { 0.0 };
                if !value.is_finite() || (value - expected).abs() > 1e-8 {
                    return Err(invalid("camera rotation must be orthonormal"));
                }
            }
        }
        if (self.world_to_camera.det() - 1.0).abs() > 1e-8 {
            return Err(invalid("camera rotation must be right-handed (determinant +1)"));
        }
        Ok(())
    }

    /// Project a metric world point into the oriented image.
    pub fn project(&self, point_m: Vec3) -> Result<Vec2, Refusal> {
        self.validate()?;
        self.intrinsics.project(self.world_to_camera.mul_vec(point_m - self.center_m))
    }

    /// Unit world-space ray from this camera's optical center through an oriented pixel.
    pub fn ray(&self, pixel: Vec2) -> Result<Vec3, Refusal> {
        self.validate()?;
        Ok(self.world_to_camera.transpose().mul_vec(self.intrinsics.bearing(pixel)?).normalized())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intrinsics() -> Intrinsics {
        Intrinsics {
            focal_px: Vec2::new(800.0, 900.0),
            principal_px: Vec2::new(320.0, 240.0),
            distortion: Distortion::default(),
        }
    }

    #[test]
    fn exact_pinhole_and_z_up_world_convention() {
        let camera = Camera {
            intrinsics: intrinsics(),
            world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
            center_m: Vec3::new(0.0, -3.0, 1.0),
        };
        let point = Vec3::new(0.3, 0.0, 1.6);
        let pixel = camera.project(point).unwrap();
        assert!((pixel - Vec2::new(400.0, 60.0)).norm() < 1e-12);
        assert!((camera.ray(pixel).unwrap() - (point - camera.center_m).normalized()).norm() < 1e-12);
        assert!(camera.project(Vec3::new(0.0, -4.0, 1.0)).is_err());
    }

    #[test]
    fn radial_tangential_round_trip_across_calibrated_disk() {
        for k1 in [-0.02, 0.0, 0.15] {
            let lens = Distortion {
                radial: [k1, 0.004, 0.001],
                tangential: [0.002, -0.003],
                max_radius: 2.0,
            };
            for x in -6..=6 {
                for y in -6..=6 {
                    let point = Vec2::new(x as f64 / 5.0, y as f64 / 5.0);
                    let got = lens.undistort(lens.distort(point).unwrap()).unwrap();
                    assert!((point - got).norm() < 2e-12, "{point:?}: {got:?}");
                }
            }
        }
    }

    #[test]
    fn distortion_has_known_answer_and_analytic_jacobian() {
        let lens = Distortion {
            radial: [0.1, 0.0, 0.0],
            tangential: [0.002, -0.003],
            max_radius: 2.0,
        };
        let point = Vec2::new(0.2, -0.1);
        let (value, [a, b, c]) = lens.evaluate(point);
        assert!((value - Vec2::new(0.20053, -0.10024)).norm() < 1e-14);
        let h = 1e-6;
        let dx = (lens.evaluate(point + Vec2::new(h, 0.0)).0
            - lens.evaluate(point - Vec2::new(h, 0.0)).0) / (2.0 * h);
        let dy = (lens.evaluate(point + Vec2::new(0.0, h)).0
            - lens.evaluate(point - Vec2::new(0.0, h)).0) / (2.0 * h);
        assert!((dx - Vec2::new(a, b)).norm() < 1e-9);
        assert!((dy - Vec2::new(b, c)).norm() < 1e-9);
    }

    #[test]
    fn malformed_and_ambiguous_calibrations_fail_closed() {
        let folding = Distortion { radial: [-1.0, 0.0, 0.0], ..Distortion::default() };
        assert!(folding.undistort(Vec2::ZERO).is_err());
        assert!(Distortion::default().undistort(Vec2::new(3.0, 0.0)).is_err());
        assert!(Distortion::default().distort(Vec2::new(f64::NAN, 0.0)).is_err());
        let mut camera = Camera {
            intrinsics: intrinsics(),
            world_to_camera: Mat3::IDENTITY,
            center_m: Vec3::ZERO,
        };
        camera.world_to_camera.rows[0][0] = -1.0;
        assert!(camera.validate().is_err());
        camera.world_to_camera = Mat3::IDENTITY;
        camera.intrinsics.focal_px.x = 0.0;
        assert!(camera.ray(Vec2::ZERO).is_err());
    }
}
