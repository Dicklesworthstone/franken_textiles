//! Calibrated sheet poses and analytic derivatives for the shared camera model (FTX-20).
//! A stationary sheet supplies the common metric gauge, not an independently movable frame.

use ftx_core::{Mat3, Refusal, RefusalCode, Vec2, Vec3};

pub use crate::camera::Camera;
use crate::camera::Intrinsics;

use super::geometry;

fn geometry_error(message: &str) -> Refusal {
    Refusal::new(RefusalCode::DegenerateGeometry, message)
        .repair("check camera calibration, coordinate frames and the metric reference")
}

impl Camera {
    /// Recover a metric pose from known intrinsics and 4–512 paired sheet points.
    /// `sheet_xz_m` defines the SAME stationary sheet frame in every image: world (x, 0, z),
    /// with x right and z up. Image points may be distorted; they are undistorted before fitting.
    /// The sheet coordinates fix metric scale. Residuals over 3 px and non-rigid homographies
    /// are refused. This algebraic initialization is not bundle adjustment or a noisy-PnP oracle.
    pub fn from_sheet(
        intrinsics: Intrinsics,
        sheet_xz_m: &[Vec2],
        pixels: &[Vec2],
    ) -> Result<Self, Refusal> {
        if !(4..=512).contains(&sheet_xz_m.len()) || sheet_xz_m.len() != pixels.len() {
            return Err(geometry_error(
                "sheet pose needs 4–512 paired metric and image points",
            ));
        }
        intrinsics.validate()?;
        let source: Vec<[f64; 2]> = sheet_xz_m.iter().map(|p| (*p).into()).collect();
        let normalized: Vec<[f64; 2]> = pixels
            .iter()
            .map(|p| intrinsics.bearing(*p).map(|q| [q.x / q.z, q.y / q.z]))
            .collect::<Result<_, _>>()?;
        let h = Mat3::from_rows(geometry::fit(&source, &normalized)?.0);
        let (a, b, t) = (h.col(0), h.col(1), h.col(2));
        let (na, nb) = (a.norm(), b.norm());
        if !na.is_finite()
            || !nb.is_finite()
            || na == 0.0
            || nb == 0.0
            || (na - nb).abs() / na.max(nb) > 0.025
            || (a / na).dot(b / nb).abs() > 0.025
            || !t.is_finite()
            || t.z == 0.0
        {
            return Err(geometry_error(
                "sheet homography is inconsistent with a calibrated rigid camera",
            ));
        }
        let sign = t.z.signum();
        let rx = a / na * sign;
        let rz = (b - rx * rx.dot(b)).normalized() * sign;
        let rotation = Mat3::from_cols(rx, rz.cross(rx), rz);
        let translation = t * (sign * 2.0 / (na + nb));
        let camera = Self {
            intrinsics,
            world_to_camera: rotation,
            center_m: -rotation.transpose().mul_vec(translation),
        };
        camera.validate()?;
        for (p, observed) in sheet_xz_m.iter().zip(pixels) {
            let error = camera.project(Vec3::new(p.x, 0.0, p.y))?.dist(*observed);
            if !error.is_finite() || error > 3.0 {
                return Err(geometry_error("sheet pose reprojection error exceeds 3 px"));
            }
        }
        Ok(camera)
    }

    /// Projection and its exact 2-by-3 derivative with respect to metric world position.
    /// Validity and the lens domain are checked by the same projection used throughout the crate.
    pub fn project_with_jacobian(&self, world_m: Vec3) -> Result<(Vec2, [[f64; 3]; 2]), Refusal> {
        let pixel = self.project(world_m)?;
        let q = self.world_to_camera.mul_vec(world_m - self.center_m);
        let p = Vec2::new(q.x / q.z, q.y / q.z);
        let [k1, k2, k3] = self.intrinsics.distortion.radial;
        let [p1, p2] = self.intrinsics.distortion.tangential;
        let r2 = p.norm2();
        let radial = 1.0 + r2 * (k1 + r2 * (k2 + r2 * k3));
        let slope = k1 + r2 * (2.0 * k2 + 3.0 * r2 * k3);
        let cross = 2.0 * p.x * p.y * slope + 2.0 * p1 * p.x + 2.0 * p2 * p.y;
        let j = [
            [
                radial + 2.0 * p.x * p.x * slope + 2.0 * p1 * p.y + 6.0 * p2 * p.x,
                cross,
            ],
            [
                cross,
                radial + 2.0 * p.y * p.y * slope + 6.0 * p1 * p.y + 2.0 * p2 * p.x,
            ],
        ];
        let normalized: [[f64; 3]; 2] = std::array::from_fn(|i| {
            std::array::from_fn(|k| {
                (self.world_to_camera.rows[i][k] - [p.x, p.y][i] * self.world_to_camera.rows[2][k])
                    / q.z
            })
        });
        let derivative: [[f64; 3]; 2] = std::array::from_fn(|i| {
            std::array::from_fn(|k| {
                [self.intrinsics.focal_px.x, self.intrinsics.focal_px.y][i]
                    * (j[i][0] * normalized[0][k] + j[i][1] * normalized[1][k])
            })
        });
        if !derivative.iter().flatten().all(|x| x.is_finite()) {
            return Err(geometry_error("camera projection derivative overflow"));
        }
        Ok((pixel, derivative))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Distortion;

    fn camera() -> Camera {
        Camera {
            intrinsics: Intrinsics {
                focal_px: Vec2::new(1400.0, 1380.0),
                principal_px: Vec2::new(640.0, 480.0),
                distortion: Distortion {
                    radial: [0.04, 0.001, 0.0],
                    tangential: [0.001, -0.001],
                    max_radius: 2.0,
                },
            },
            world_to_camera: Mat3::rotation(Vec3::new(0.0, 1.0, 0.0), 0.25)
                * Mat3::rotation(Vec3::new(1.0, 0.0, 0.0), std::f64::consts::FRAC_PI_2),
            center_m: Vec3::new(-0.1, -2.1, 0.2),
        }
    }

    #[test]
    fn projection_derivatives_match_central_differences() {
        let c = camera();
        let point = Vec3::new(0.25, -0.1, 0.6);
        let (pixel, j) = c.project_with_jacobian(point).unwrap();
        assert_eq!(pixel, c.project(point).unwrap());
        for k in 0..3 {
            let delta = Vec3::from(std::array::from_fn(|i| if i == k { 1e-6 } else { 0.0 }));
            let d = (c.project(point + delta).unwrap() - c.project(point - delta).unwrap()) / 2e-6;
            assert!((d.x - j[0][k]).abs() < 1e-6);
            assert!((d.y - j[1][k]).abs() < 1e-6);
        }
        assert!(c.project_with_jacobian(Vec3::new(0.0, -4.0, 0.0)).is_err());
    }

    #[test]
    fn sheet_recovers_metric_pose_and_off_plane_projection() {
        let c = camera();
        let sheet = [
            Vec2::new(0.0, 0.0),
            Vec2::new(0.21, 0.0),
            Vec2::new(0.21, 0.297),
            Vec2::new(0.0, 0.297),
            Vec2::new(0.105, 0.1485),
        ];
        let pixels: Vec<_> = sheet
            .iter()
            .map(|p| c.project(Vec3::new(p.x, 0.0, p.y)).unwrap())
            .collect();
        let recovered = Camera::from_sheet(c.intrinsics, &sheet, &pixels).unwrap();
        assert!((recovered.center_m - c.center_m).norm() < 1e-8);
        let held_out = Vec3::new(0.4, -0.3, 0.8);
        assert!(
            recovered
                .project(held_out)
                .unwrap()
                .dist(c.project(held_out).unwrap())
                < 1e-6
        );
        // Metric scale comes solely from the sheet, never a population/body-size prior.
        let doubled: Vec<_> = sheet.iter().map(|p| *p * 2.0).collect();
        let scaled = Camera::from_sheet(c.intrinsics, &doubled, &pixels).unwrap();
        assert!((scaled.center_m - c.center_m * 2.0).norm() < 1e-8);
        let wrong: Vec<_> = sheet.iter().map(|p| Vec2::new(p.x * 1.5, p.y)).collect();
        assert!(Camera::from_sheet(c.intrinsics, &wrong, &pixels).is_err());
        assert!(Camera::from_sheet(c.intrinsics, &[Vec2::ZERO; 4], &[Vec2::ZERO; 4]).is_err());
    }
}
