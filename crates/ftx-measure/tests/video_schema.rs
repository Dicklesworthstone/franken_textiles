//! The decoded acquisition schema uses the core's array-valued vector wire format.
use ftx_core::Vec2;
use ftx_measure::video::{FrameMetadata, SelectionOptions, score_frame};
use serde_json::json;

#[test]
fn acquisition_metadata_round_trips_without_losing_focal_lengths() {
    let value = json!({
        "frame":{"stream":"synthetic","timestamp_us":133333},
        "rotation_cw_deg":90,"camera_epoch":"lens-a","focal_px":[1500.0,1450.0]
    });
    let metadata: FrameMetadata = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(metadata.focal_px, Some(Vec2::new(1500.0, 1450.0)));
    assert_eq!(serde_json::to_value(&metadata).unwrap(), value);
    let quality = score_frame(metadata, [3, 3], &[80; 9], &[255; 9], &[255; 9]).unwrap();
    assert_eq!(serde_json::to_value(quality).unwrap()["metadata"], value);
    let mut object_vector = value;
    object_vector["focal_px"] = json!({"x":1500.0,"y":1450.0});
    assert!(serde_json::from_value::<FrameMetadata>(object_vector).is_err());
}

#[test]
fn documented_manifest_uses_the_actual_metadata_and_policy_schema() {
    let docs = include_str!("../../../docs/video-frame-selection.md");
    let block = docs
        .split_once("## Decoded input manifest\n")
        .unwrap()
        .1
        .split("```json\n")
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap();
    let input: serde_json::Value = serde_json::from_str(block).unwrap();
    assert_eq!(input["schema"], "ftx-video-frames/1");
    let options: SelectionOptions = serde_json::from_value(input["options"].clone()).unwrap();
    options.validate().unwrap();
    let frames = input["frames"].as_array().unwrap();
    assert!(!frames.is_empty());
    for frame in frames {
        let metadata: FrameMetadata = serde_json::from_value(frame["metadata"].clone()).unwrap();
        score_frame(metadata, [3, 3], &[80; 9], &[255; 9], &[255; 9]).unwrap();
    }
}
