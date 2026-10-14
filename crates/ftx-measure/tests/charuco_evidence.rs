//! Rejected pixels cannot count as evidence of a reliable calibration.
use ftx_measure::photo::analyze_reference;
use serde_json::{Value, json};

#[test]
fn reliability_warnings_count_consensus_not_raw_detections() {
    // Ten exact, well-spread corners and four unrelated false detections.
    let ids = [0, 2, 4, 6, 10, 14, 20, 28, 30, 34, 1, 8, 18, 25];
    let corners: Vec<Value> = ids
        .iter()
        .enumerate()
        .map(|(i, &id)| {
            let p = if i < 10 {
                [
                    100.0 + (id % 7 + 1) as f64 * 44.0,
                    100.0 + (id / 7 + 1) as f64 * 44.0,
                ]
            } else {
                [800.0 + 27.0 * i as f64, 800.0 - 11.0 * i as f64]
            };
            json!({"id":id,"point_px":p})
        })
        .collect();
    let reference = json!({"schema":"ftx-photo-reference/1","reference":"charuco",
        "image_size_px":[1500,1500],"board":{"squares_x":8,"squares_y":6,"square_mm":22},"corners":corners});
    let scale = analyze_reference(&reference).unwrap();
    let report = scale.report();
    assert_eq!(report["ok"], true, "{report}");
    assert_eq!(report["corners_detected"], 14);
    assert_eq!(report["inliers"], 10);
    assert!(
        report["warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|w| w.as_str().unwrap().starts_with("TOO_FEW_CORNERS: 10")),
        "{report}"
    );
    assert_eq!(report["outlier_ids"], json!([1, 8, 18, 25]));
}
