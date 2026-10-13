//! The common metric sheet gauge must survive camera recovery and off-plane triangulation.

use ftx_core::{Mat3, Vec2, Vec3};
use ftx_measure::camera::{Camera, Distortion, Intrinsics};
use ftx_measure::triangulate::{Capture, FrameId, Observation, Partition, TriangulationInput, triangulate};

#[test]
fn stationary_sheet_cameras_recover_an_off_sheet_landmark() {
    let intrinsics = Intrinsics {
        focal_px: Vec2::new(900.0, 900.0),
        principal_px: Vec2::new(500.0, 400.0),
        distortion: Distortion::default(),
    };
    let sheet = [
        Vec2::new(0.0, 0.0),
        Vec2::new(0.21, 0.0),
        Vec2::new(0.21, 0.297),
        Vec2::new(0.0, 0.297),
        Vec2::new(0.105, 0.1485),
    ];
    let point = Vec3::new(0.02, -0.2, 1.3);
    let mut observations = Vec::new();
    for (i, x) in [-0.4, 0.4, 0.0].into_iter().enumerate() {
        let known = Camera {
            intrinsics,
            world_to_camera: Mat3::from_rows([[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]),
            center_m: Vec3::new(x, -3.0, 1.0),
        };
        let pixels: Vec<_> = sheet
            .iter()
            .map(|p| known.project(Vec3::new(p.x, 0.0, p.y)).unwrap())
            .collect();
        let camera = Camera::from_sheet(intrinsics, &sheet, &pixels).unwrap();
        observations.push(Observation {
            frame: FrameId {
                stream: format!("synthetic-camera-{i}"),
                timestamp_us: 1_000_000,
            },
            camera,
            pixel_px: known.project(point).unwrap(),
            partition: if i == 2 { Partition::HeldOut } else { Partition::Fit },
        });
    }
    let input = TriangulationInput {
        schema: "ftx-triangulation-input/1".into(),
        metric_scale_source: "Synthetic common stationary 210x297 mm sheet".into(),
        capture: Capture::Synchronized {
            max_skew_us: 0,
            evidence: "Synthetic simultaneous stationary acquisition".into(),
        },
        observations,
    };
    let report = triangulate(&input).unwrap();
    assert_eq!(report.status, "ACCEPTED", "{:?}", report.flags);
    assert!((report.point_m.unwrap() - point).norm() < 1e-8);
    assert_eq!(report.held_out_views, 1);
}
