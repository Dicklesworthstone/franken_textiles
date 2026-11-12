//! Shared stationary ChArUco geometry for native and portable body-capture cameras.
//! The caller freezes the original sidecar and CHECK IDs before detecting any image.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, Vec2};
use serde_json::{Value, json};

use crate::invalid;

/// One explicit upright board at world y=0, with printed right/up defining +x/+z.
/// This does not verify print scale, gravity alignment or physical stationarity.
pub struct StationarySheet {
    columns: usize,
    rows: usize,
    square_m: f64,
    held: BTreeSet<usize>,
    origin: Vec2,
}

impl StationarySheet {
    /// Validate source board geometry and freeze independent CHECK corner identities.
    ///
    /// # Errors
    /// Invalid dimensions, scale, world origin or evidence; duplicate/out-of-range CHECK IDs;
    /// fewer than four CHECK or four remaining FIT corners; or more than 512 board corners.
    pub fn new(
        sheet: &Value,
        held_out_corner_ids: &[usize],
        board_bottom_left_xz_m: Vec2,
        stationary_upright_evidence: &str,
    ) -> Result<Self, Refusal> {
        if !board_bottom_left_xz_m.is_finite()
            || stationary_upright_evidence.trim().is_empty()
            || stationary_upright_evidence.len() > 4096
            || stationary_upright_evidence.contains('\0')
        {
            return Err(invalid(
                "ChArUco capture needs a finite board bottom-left and explicit stationary upright reference evidence",
            ));
        }
        let count = |key: &str| {
            sheet["board"][key]
                .as_u64()
                .filter(|n| (3..=64).contains(n))
                .map(|n| n as usize)
                .ok_or_else(|| invalid("ChArUco sidecar needs integer board dimensions in 3..=64"))
        };
        let (columns, rows) = (count("squares_x")? - 1, count("squares_y")? - 1);
        let total = columns * rows;
        let held: BTreeSet<_> = held_out_corner_ids.iter().copied().collect();
        if total > 512
            || held.len() != held_out_corner_ids.len()
            || held.len() < 4
            || held.iter().any(|&id| id >= total)
            || total - held.len() < 4
        {
            return Err(invalid(
                "declare at least four unique in-range held_out_corner_ids, leaving at least four FIT corners on a board with at most 512 corners",
            ));
        }
        let square_m = sheet["board"]["square_mm"].as_f64().unwrap_or(f64::NAN) / 1000.0;
        if !square_m.is_finite() || square_m <= 0.0 {
            return Err(invalid("ChArUco square_mm must be finite and positive"));
        }
        Ok(Self {
            columns,
            rows,
            square_m,
            held,
            origin: board_bottom_left_xz_m,
        })
    }

    /// Convert all detected original corner pixels to the existing sheet-pose input.
    /// Every predeclared CHECK must be present; no residual-dependent trimming is allowed.
    /// The checked calibration supplies intrinsics separately to Camera::sheet_pose_report.
    ///
    /// # Errors
    /// Missing, duplicate or out-of-range detected IDs, an absent CHECK, or fewer than four FIT.
    pub fn reference(
        &self,
        detected: &Value,
        reference_id: &str,
        image: &str,
        image_size_px: [usize; 2],
        rotation_cw_deg: u16,
    ) -> Result<Value, Refusal> {
        let corners = detected["reference"]["corners"]
            .as_array()
            .ok_or_else(|| invalid("ChArUco detector returned no corner array"))?;
        let total = self.columns * self.rows;
        let mut seen = BTreeSet::new();
        let mut points = Vec::with_capacity(corners.len());
        for corner in corners {
            let id = corner["id"]
                .as_u64()
                .and_then(|n| usize::try_from(n).ok())
                .filter(|id| *id < total && seen.insert(*id))
                .ok_or_else(|| {
                    invalid("ChArUco detector returned duplicate or out-of-range corners")
                })?;
            // Internal IDs are row-major from the printed top-left. The explicit outer
            // bottom-left fixes metric origin; page margins never participate.
            let xz = Vec2::new(
                self.origin.x + (id % self.columns + 1) as f64 * self.square_m,
                self.origin.y + (self.rows - id / self.columns) as f64 * self.square_m,
            );
            points.push(json!({"sheet_xz_m":xz,"pixel_px":corner["point_px"],
                "held_out":self.held.contains(&id)}));
        }
        if !self.held.is_subset(&seen) || seen.len() - self.held.len() < 4 {
            return Err(Refusal::new(
                RefusalCode::Unscaled,
                "ChArUco capture needs every predeclared CHECK corner and at least four detected FIT corners",
            )
            .repair("retake a sharp unobstructed board; retain the original FIT/CHECK assignments"));
        }
        Ok(
            json!({"schema":"ftx-sheet-pose-input/1","reference_id":reference_id,
            "image":image,"image_size_px":image_size_px,
            "rotation_applied_degrees":rotation_cw_deg,"points":points}),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Distortion, Intrinsics};
    use ftx_core::{Mat3, Vec3};

    fn source() -> (StationarySheet, Value, Camera) {
        let sheet = json!({"board":{"squares_x":5,"squares_y":4,"square_mm":50.0}});
        let origin = Vec2::new(-0.1, 0.2);
        let camera = Camera {
            intrinsics: Intrinsics {
                focal_px: Vec2::new(900.0, 900.0),
                principal_px: Vec2::new(320.0, 240.0),
                distortion: Distortion::default(),
            },
            world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
            center_m: Vec3::new(0.0, -2.0, 0.3),
        };
        let corners: Vec<_> = (0..12)
            .map(|id| {
                let point = Vec3::new(
                    -0.1 + (id % 4 + 1) as f64 * 0.05,
                    0.0,
                    0.2 + (3 - id / 4) as f64 * 0.05,
                );
                json!({"id":id,"point_px":camera.project(point).unwrap()})
            })
            .collect();
        (
            StationarySheet::new(
                &sheet,
                &[0, 3, 8, 11],
                origin,
                "synthetic fixed upright board",
            )
            .unwrap(),
            json!({"reference":{"corners":corners}}),
            camera,
        )
    }

    fn pose(sheet: &StationarySheet, detected: &Value, camera: Camera) -> Value {
        let mut reference = sheet
            .reference(detected, "synthetic", "frame.png", [640, 480], 0)
            .unwrap();
        reference["intrinsics"] = json!(camera.intrinsics);
        Camera::sheet_pose_report(&reference).unwrap()
    }

    #[test]
    fn printed_coordinates_recover_the_source_camera_and_do_not_fit_check_points() {
        let (sheet, mut detected, camera) = source();
        let result = pose(&sheet, &detected, camera);
        assert_eq!(result["ok"], true);
        assert_eq!(result["fit_points"], 8);
        assert_eq!(result["held_out_points"], 4);
        let recovered: Camera = serde_json::from_value(result["camera"].clone()).unwrap();
        assert!((recovered.center_m - camera.center_m).norm() < 1e-10);
        assert!(
            recovered
                .project(Vec3::new(0.15, -0.1, 0.4))
                .unwrap()
                .dist(camera.project(Vec3::new(0.15, -0.1, 0.4)).unwrap())
                < 1e-8
        );
        let pixel = detected["reference"]["corners"][0]["point_px"][0]
            .as_f64()
            .unwrap();
        detected["reference"]["corners"][0]["point_px"][0] = json!(pixel + 0.25);
        let shifted = pose(&sheet, &detected, camera);
        assert_eq!(shifted["camera"], result["camera"]);
        detected["reference"]["corners"][0]["point_px"][0] = json!(pixel + 4.0);
        let refused = pose(&sheet, &detected, camera);
        assert_eq!(refused["ok"], false);
        assert!(refused["camera"].is_null());
    }

    #[test]
    fn predeclared_check_ids_cannot_be_missing_repeated_or_reassigned() {
        let (sheet, detected, _) = source();
        let mut missing = detected.clone();
        missing["reference"]["corners"]
            .as_array_mut()
            .unwrap()
            .remove(0);
        assert!(sheet.reference(&missing, "r", "i", [640, 480], 0).is_err());
        let mut repeated = detected;
        repeated["reference"]["corners"][1]["id"] = json!(0);
        assert!(sheet.reference(&repeated, "r", "i", [640, 480], 0).is_err());
        let board = json!({"board":{"squares_x":5,"squares_y":4,"square_mm":50.0}});
        for held in [vec![0, 0, 8, 11], vec![0, 3, 8, 12], vec![0, 3, 8]] {
            assert!(StationarySheet::new(&board, &held, Vec2::ZERO, "declared").is_err());
        }
        assert!(StationarySheet::new(&board, &[0, 3, 8, 11], Vec2::ZERO, "").is_err());
    }
}
