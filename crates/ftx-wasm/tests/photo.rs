//! Synthetic sheet-plane answers and the actual shared photo engines; no real-camera claim.
use ftx_core::receipt::sha256_hex;
use ftx_measure::photo::{PhotoOptions, analyze_plain_sheet, measure_photo};
use ftx_trace::raster::{
    Gray,
    image::{encode_png_gray, load_photo_with_limit, load_png},
};
use ftx_wasm::photo::Photo;
use serde_json::json;

fn fixture() -> Vec<u8> {
    let mut image = Gray::filled(720, 960, 90);
    for y in 0..image.h {
        for x in 0..image.w {
            let dx =
                ((x as f64 + 0.5).min(150.2 + 215.9 * 1.5) - (x as f64 - 0.5).max(150.2)).max(0.0);
            let dy =
                ((y as f64 + 0.5).min(170.4 + 279.4 * 1.5) - (y as f64 - 0.5).max(170.4)).max(0.0);
            image.data[y * image.w + x] = (90.0 + 152.0 * dx * dy).round() as u8;
        }
    }
    encode_png_gray(&image)
}

#[test]
fn photo_uses_native_unrounded_scale_records_and_exact_preview_pixels() {
    let bytes = fixture();
    let photo = Photo::new(&bytes, "letter").unwrap();
    let (image, receipt) = load_photo_with_limit(&bytes, 16_000_000).unwrap();
    let native = analyze_plain_sheet(&image.data, image.w, image.h, "letter").unwrap();
    let mut report = photo.report();
    assert_eq!(report["image_input"], receipt);
    assert_eq!(receipt["sha256"], sha256_hex(&bytes));
    report.as_object_mut().unwrap().remove("image_input");
    assert_eq!(&report, native.report());
    assert!(report["ok"].as_bool().unwrap());
    assert!((report["mm_per_px"].as_f64().unwrap() - 2.0 / 3.0).abs() < 0.002);
    assert_eq!(load_png(&photo.preview().unwrap()).unwrap(), image);
    let points = json!({"across_back":[[30,400],[630,400]]});
    let actual = photo.measure(&points.to_string(), "{}").unwrap();
    let expected = measure_photo(&native, &points, &PhotoOptions::default()).unwrap();
    assert_eq!(actual["records"], expected["records"]);
    assert!((actual["records"][0]["value_mm"].as_f64().unwrap() - 400.0).abs() < 1.0);
    assert!(actual["records"][0]["sigma_mm"].as_f64().unwrap() > 0.0);
    assert_eq!(actual["records"][0]["provenance"], "PHOTO_ESTIMATED");
    assert_eq!(actual["schema"], "fabric-measurements/1");
    assert_eq!(actual["taken_on"], serde_json::Value::Null);
    assert_eq!(actual, photo.measure(&points.to_string(), "{}").unwrap());
    assert!(
        photo
            .measure(r#"{"across_back":[[30,400],[730,400]]}"#, "{}")
            .is_err()
    );
    assert!(
        photo
            .measure(&points.to_string(), r#"{"bootstrap":19}"#)
            .is_err()
    );
    assert!(
        photo
            .measure(&points.to_string(), r#"{"unknown":1}"#)
            .is_err()
    );
}

#[test]
fn jpeg_orientation_and_preview_share_the_native_evidence_frame() {
    let bytes = include_bytes!("../../../tests/fixtures/photo/plain-letter-exif6.jpg");
    let (image, receipt) = load_photo_with_limit(bytes, 16_000_000).unwrap();
    let photo = Photo::new(bytes, "letter").unwrap();
    assert_eq!(receipt["exif_orientation"], 6);
    assert_eq!(photo.report()["image_input"], receipt);
    assert_eq!(load_png(&photo.preview().unwrap()).unwrap(), image);
    let native = analyze_plain_sheet(&image.data, image.w, image.h, "letter").unwrap();
    let points = json!({"across_back":[[30,400],[530,400]]});
    assert_eq!(
        photo.measure(&points.to_string(), "{}").unwrap()["records"],
        measure_photo(&native, &points, &PhotoOptions::default()).unwrap()["records"]
    );
}

#[test]
fn rejected_calibration_and_images_publish_no_measurements() {
    let photo = Photo::new(&fixture(), "a4").unwrap();
    assert_eq!(photo.report()["ok"], false);
    assert!(
        photo
            .measure(r#"{"across_back":[[30,400],[630,400]]}"#, "{}")
            .is_err()
    );
    assert!(Photo::new(b"not an image", "auto").is_err());
    assert!(Photo::new(&fixture(), "unknown").is_err());
    assert!(load_photo_with_limit(&fixture(), 10).is_err());
    let blank = Photo::new(&encode_png_gray(&Gray::filled(100, 100, 90)), "auto").unwrap();
    assert_eq!(blank.report()["ok"], false);
    assert!(
        blank
            .measure(r#"{"test":[[20,20],[50,50]]}"#, "{}")
            .is_err()
    );
}
