//! Exact synthetic camera input. No photographs or private measurements.
use ftx_core::{Vec2, Vec3};
use ftx_measure::camera::Camera;
use serde_json::{Value, json};

fn input() -> Value {
    json!({"schema":"ftx-sheet-pose-input/1", "reference_id":"synthetic-stationary-a4",
    "intrinsics":{"focal_px":[1000,1000],"principal_px":[640,480]},
    "image_size_px":[1280,1200],"rotation_applied_degrees":0,
    "points":[
        {"sheet_xz_m":[0,0],"pixel_px":[640,980]},
        {"sheet_xz_m":[0.21,0],"pixel_px":[745,980]},
        {"sheet_xz_m":[0.21,0.297],"pixel_px":[745,831.5]},
        {"sheet_xz_m":[0,0.297],"pixel_px":[640,831.5]},
        {"sheet_xz_m":[0.105,0.1485],"pixel_px":[692.5,905.75],"held_out":true}
    ]})
}

#[test]
fn report_yields_the_shared_metric_camera_with_independent_checks() {
    let v = input();
    let report = Camera::sheet_pose_report(&v).unwrap();
    assert_eq!(report["ok"], true);
    assert_eq!(report["input"], v);
    assert_eq!(report["fit_points"], 4);
    assert_eq!(report["held_out_points"], 1);
    let c: Camera = serde_json::from_value(report["camera"].clone()).unwrap();
    assert!((c.center_m - Vec3::new(0.0, -2.0, 1.0)).norm() < 1e-9);
    assert!(
        (c.project(Vec3::new(0.3, -0.5, 0.5)).unwrap() - Vec2::new(840.0, 480.0 + 1000.0 / 3.0))
            .norm()
            < 1e-7
    );
    assert!(
        report["reprojection"]
            .as_array()
            .unwrap()
            .iter()
            .all(|r| r["error_px"].as_f64().unwrap() < 1e-8)
    );
    assert_eq!(report, Camera::sheet_pose_report(&v).unwrap());
}

#[test]
fn held_out_points_never_steer_calibration_or_publish_a_failed_camera() {
    let mut v = input();
    let original = Camera::sheet_pose_report(&v).unwrap();
    v["points"][4]["pixel_px"][1] = json!(907.75);
    let small = Camera::sheet_pose_report(&v).unwrap();
    assert_eq!(small["ok"], true);
    assert_eq!(small["camera"], original["camera"]);
    v["points"][4]["pixel_px"][1] = json!(909.75);
    let bad = Camera::sheet_pose_report(&v).unwrap();
    assert_eq!(bad["ok"], false);
    assert!(bad["camera"].is_null());
    assert_eq!(bad["flags"].as_array().unwrap().len(), 1);
    assert!(bad["reprojection"][4]["error_px"].as_f64().unwrap() > 3.9);
}

#[test]
fn malformed_identity_coordinates_and_partitions_fail_closed() {
    for mutate in [
        |v: &mut Value| v["schema"] = json!("unknown"),
        |v: &mut Value| v["reference_id"] = json!(" "),
        |v: &mut Value| v["points"][0]["pixel_px"][0] = json!(-1),
        |v: &mut Value| v["points"][1]["pixel_px"] = v["points"][0]["pixel_px"].clone(),
        |v: &mut Value| v["points"][4]["sheet_xz_m"] = v["points"][0]["sheet_xz_m"].clone(),
        |v: &mut Value| v["points"][0]["held_out"] = json!(true),
        |v: &mut Value| v["rotation_applied_degrees"] = json!(45),
        |v: &mut Value| v["image_size_px"] = json!([0, 1200]),
        |v: &mut Value| v["intrinsics"]["focla_px"] = json!([1000, 1000]),
        |v: &mut Value| v["points"][0]["hold_out"] = json!(true),
    ] {
        let mut v = input();
        mutate(&mut v);
        assert!(Camera::sheet_pose_report(&v).is_err(), "accepted {v}");
    }
}
