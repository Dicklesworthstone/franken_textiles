//! The trace's raster operations against `tests/fixtures/oracle/trace/raster_*.json` (captured by
//! `tools/oracle/capture_raster.py` from the reference's Pillow / OpenCV / numpy): every value bit for bit.
//! - `raster_image`: `load_image` on PNGs of every bit depth and colour type (all filters, Adam7, palette and
//!   gray/RGB transparency, partial alpha) → the BGR array.

use ftx_conformance::fixture;
use ftx_trace::raster::{
    Bgr, bgr_saturation, bgr_to_gray, bgr_to_lab, lab, lab_distance, load_png,
};
use serde_json::Value;

fn bytes(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_u64().unwrap() as u8)
        .collect()
}

fn base64(s: &str) -> Vec<u8> {
    let val = |c: u8| -> u32 {
        match c {
            b'A'..=b'Z' => u32::from(c - b'A'),
            b'a'..=b'z' => u32::from(c - b'a') + 26,
            b'0'..=b'9' => u32::from(c - b'0') + 52,
            b'+' => 62,
            _ => 63,
        }
    };
    let bytes: Vec<u8> = s.bytes().filter(|&c| c != b'=').collect();
    let mut out = Vec::new();
    for chunk in bytes.chunks(4) {
        let n = chunk.iter().fold(0u32, |acc, &c| (acc << 6) | val(c)) << (6 * (4 - chunk.len()));
        let take = chunk.len() * 6 / 8;
        out.extend(&n.to_be_bytes()[1..=take]);
    }
    out
}

#[test]
fn png_loading_matches_pillow() {
    let fx = fixture("trace", "raster_image");
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let bytes = base64(case["input"]["png"].as_str().unwrap());
        let out = &case["output"];
        match load_png(&bytes) {
            Ok(img) => {
                let shape = [img.h, img.w, 3];
                let want: Vec<u8> = out["bgr"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u8)
                    .collect();
                let want_shape: Vec<usize> = out["shape"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as usize)
                    .collect();
                if shape[..] != want_shape[..] {
                    fails.push(format!("{id}: shape {shape:?} vs {want_shape:?}"));
                } else if img.data != want {
                    let first = img
                        .data
                        .iter()
                        .zip(&want)
                        .position(|(a, b)| a != b)
                        .unwrap();
                    fails.push(format!(
                        "{id}: byte {first}: {} vs {}",
                        img.data[first], want[first]
                    ));
                }
            }
            Err(e) => fails.push(format!("{id}: refused: {e}")),
        }
    }
    assert!(
        fails.is_empty(),
        "{} mismatches:\n{}",
        fails.len(),
        fails.join("\n")
    );
}

/// Lab over every 8-bit colour bit for bit; the `auto` channel bit for bit; gray and HSV saturation, which the
/// reference's arm64 OpenCV runs through its KleidiCV/carotene HAL, within one level on under 1 % of pixels.
#[test]
fn colour_conversions_match_opencv() {
    let fx = fixture("trace", "raster_color");
    let cases = fx["cases"].as_array().unwrap();
    let sample = Bgr {
        w: 128,
        h: 128,
        data: bytes(&cases[1]["input"]["bgr"]),
    };
    let (got, want) = (bgr_to_lab(&sample).data, bytes(&cases[1]["output"]["lab"]));
    let bad: Vec<String> = (0..got.len() / 3)
        .filter(|&i| got[3 * i..3 * i + 3] != want[3 * i..3 * i + 3])
        .map(|i| {
            format!(
                "{:?} -> {:?} vs {:?}",
                &sample.data[3 * i..3 * i + 3],
                &got[3 * i..3 * i + 3],
                &want[3 * i..3 * i + 3]
            )
        })
        .collect();
    assert!(
        bad.is_empty(),
        "{} of {} Lab pixels differ:\n{}",
        bad.len(),
        got.len() / 3,
        bad[..bad.len().min(12)].join("\n")
    );
    let mut all = Vec::with_capacity(3 << 24);
    for i in 0u32..1 << 24 {
        all.extend_from_slice(&lab([
            (i & 255) as u8,
            ((i >> 8) & 255) as u8,
            (i >> 16) as u8,
        ]));
    }
    assert_eq!(
        ftx_core::receipt::sha256_hex(&all),
        cases[0]["output"]["sha256"].as_str().unwrap(),
        "Lab over every colour"
    );
    for (got, key) in [
        (bgr_to_gray(&sample).data, "gray"),
        (bgr_saturation(&sample).data, "sat"),
    ] {
        let want = bytes(&cases[1]["output"][key]);
        let off: Vec<i32> = got
            .iter()
            .zip(&want)
            .map(|(a, b)| i32::from(*a) - i32::from(*b))
            .filter(|d| *d != 0)
            .collect();
        println!(
            "{key}: {} of {} pixels differ from the HAL, by at most {}",
            off.len(),
            want.len(),
            off.iter().map(|d| d.abs()).max().unwrap_or(0)
        );
        assert!(
            off.len() * 100 <= want.len() && off.iter().all(|d| d.abs() <= 1),
            "{key}"
        );
    }
    for case in &cases[2..] {
        let bg: Vec<f64> = case["input"]["bg"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_f64().unwrap())
            .collect();
        let bg8 = [0, 1, 2].map(|k| bg[k].clamp(0.0, 255.0) as u8);
        assert_eq!(
            lab_distance(&sample, bg8).data,
            bytes(&case["output"]["channel"]),
            "{}",
            case["id"]
        );
    }
}
