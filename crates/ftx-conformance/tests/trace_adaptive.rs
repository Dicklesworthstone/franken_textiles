//! Supplementary CPU OpenCV 4.13 pixel oracle. The original OpenCV 5 full-trace fixture
//! remains the authority for skill parity; this checks kernels, borders and block sizes.

use ftx_conformance::fixture;
use ftx_core::receipt::sha256_hex;
use ftx_trace::raster::{image::Gray, threshold::adaptive_gaussian};

#[test]
fn adaptive_masks_match_independent_opencv_pixels() {
    let data = fixture("trace", "adaptive_opencv413");
    let cases = data["cases"].as_array().unwrap();
    assert_eq!(cases.len(), 24);
    for case in cases {
        let w = case["width"].as_u64().unwrap() as usize;
        let h = case["height"].as_u64().unwrap() as usize;
        let mut image = Gray::filled(w, h, 0);
        for y in 0..h {
            for x in 0..w {
                image.data[y * w + x] = match case["kind"].as_str().unwrap() {
                    "texture" => ((x * 79 + y * 37 + x * y * 11) % 256) as u8,
                    "ramp" => (x * 130 / (w - 1).max(1) + y * 80 / (h - 1).max(1)) as u8,
                    "impulse" => u8::from((x == 0 && y == 0) || (x == w / 2 && y == h / 2)) * 255,
                    other => panic!("unknown input {other}"),
                };
            }
        }
        assert_eq!(
            sha256_hex(&image.data),
            case["input_sha256"],
            "{}: input",
            case["id"]
        );
        let mask = adaptive_gaussian(&image).unwrap();
        assert_eq!(
            sha256_hex(&mask.data),
            case["mask_sha256"],
            "{}: mask",
            case["id"]
        );
        assert_eq!(
            mask.data.iter().filter(|&&v| v == 255).count(),
            case["foreground_pixels"].as_u64().unwrap() as usize
        );
    }
}
