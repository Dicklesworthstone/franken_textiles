//! The trace's raster operations against `tests/fixtures/oracle/trace/raster_*.json` (captured by
//! `tools/oracle/capture_raster.py` from the reference's Pillow / OpenCV / numpy): every value bit for bit.
//! - `raster_image`: `load_image` on PNGs of every bit depth and colour type (all filters, Adam7, palette and
//!   gray/RGB transparency, partial alpha) → the BGR array.
//! - `raster_color`: Lab over every colour, the `auto` channel; gray and saturation within the HAL's level.
//! - `raster_binarize`: Otsu thresholds and masks, elliptical structuring elements, close, open, gradient,
//!   dilate and erode.

use ftx_conformance::fixture;
use ftx_trace::raster::morph::{close, dilate, erode, gradient, open};
use ftx_trace::raster::threshold::otsu_binary;
use ftx_trace::raster::{
    Bgr, Gray, Kernel, bgr_saturation, bgr_to_gray, bgr_to_lab, lab, lab_distance, load_png,
};
use serde_json::Value;

fn gray_in(v: &Value) -> Gray {
    Gray {
        w: v["w"].as_u64().unwrap() as usize,
        h: v["h"].as_u64().unwrap() as usize,
        data: bytes(&v["data"]),
    }
}

fn pts(v: &Value) -> Vec<(i32, i32)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|p| (p[0].as_i64().unwrap() as i32, p[1].as_i64().unwrap() as i32))
        .collect()
}

/// Contours (points, order, hierarchy) and every contour function, bit for bit, on random blobs, nested
/// rings, noise and the three test sheets' real masks.
#[test]
fn contours_match_opencv() {
    use ftx_trace::raster::contours::{
        Mode, approx_poly_dp, arc_length, area, bounding_rect, find_contours, moments,
    };
    let fx = fixture("trace", "raster_contours");
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let inp = &case["input"];
        let img = match inp.get("mask_png") {
            Some(png) => {
                let bgr = load_png(&base64(png.as_str().unwrap())).unwrap();
                Gray {
                    w: bgr.w,
                    h: bgr.h,
                    data: bgr.data.iter().step_by(3).copied().collect(),
                }
            }
            None => gray_in(inp),
        };
        let mode = if inp["mode"] == "ccomp" {
            Mode::CComp
        } else {
            Mode::External
        };
        let got = find_contours(&img, mode);
        let out = &case["output"];
        let want_h: Vec<[i32; 4]> = out["hierarchy"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| [0, 1, 2, 3].map(|k| r[k].as_i64().unwrap() as i32))
            .collect();
        let rows = out["contours"].as_array().unwrap();
        if got.contours.len() != rows.len() || got.hierarchy != want_h {
            fails.push(format!(
                "{id}: {} contours vs {}, hierarchy equal {}",
                got.contours.len(),
                rows.len(),
                got.hierarchy == want_h
            ));
            continue;
        }
        for (k, (c, row)) in got.contours.iter().zip(rows).enumerate() {
            if *c != pts(&row["pts"]) {
                fails.push(format!(
                    "{id}#{k}: points differ ({} vs {})",
                    c.len(),
                    row["pts"].as_array().unwrap().len()
                ));
                continue;
            }
            let ap = approx_poly_dp(c, 0.7, true);
            let m = moments(c);
            let r = bounding_rect(c);
            let checks = [
                ("area", area(c), row["area"].as_f64().unwrap()),
                ("arc", arc_length(c, true), row["arc"].as_f64().unwrap()),
                (
                    "approx_arc",
                    arc_length(&ap, true),
                    row["approx_arc"].as_f64().unwrap(),
                ),
                ("m00", m.0, row["m"][0].as_f64().unwrap()),
                ("m10", m.1, row["m"][1].as_f64().unwrap()),
                ("m01", m.2, row["m"][2].as_f64().unwrap()),
            ];
            for (what, g, w) in checks {
                if g != w {
                    fails.push(format!("{id}#{k}: {what} {g} vs {w}"));
                }
            }
            if ap != pts(&row["approx"]) {
                fails.push(format!(
                    "{id}#{k}: approxPolyDP differs ({} vs {} points)",
                    ap.len(),
                    row["approx"].as_array().unwrap().len()
                ));
            }
            if [r.0, r.1, r.2, r.3] != [0, 1, 2, 3].map(|i| row["rect"][i].as_i64().unwrap() as i32)
            {
                fails.push(format!("{id}#{k}: boundingRect differs"));
            }
        }
    }
    let total: usize = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["output"]["contours"].as_array().unwrap().len())
        .sum();
    let points: usize = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|c| {
            c["output"]["contours"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["pts"].as_array().unwrap().len())
        })
        .sum();
    println!("{total} contours, {points} points compared");
    assert!(
        fails.is_empty(),
        "{} mismatches:\n{}",
        fails.len(),
        fails[..fails.len().min(20)].join("\n")
    );
}

#[test]
fn thresholds_and_morphology_match_opencv() {
    let fx = fixture("trace", "raster_binarize");
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let (inp, out) = (&case["input"], &case["output"]);
        let ok = if let Some(name) = id.strip_prefix("otsu-") {
            let (t, m) = otsu_binary(&gray_in(inp));
            let want = out["t"].as_f64().unwrap();
            if t != want {
                fails.push(format!("{name}: threshold {t} vs {want}"));
            }
            m.data == bytes(&out["mask"])
        } else if id.starts_with("ellipse-") {
            let k = inp["k"].as_u64().unwrap() as usize;
            let se: Vec<u8> = Kernel::ellipse(k, k)
                .on
                .iter()
                .map(|&b| u8::from(b))
                .collect();
            se == bytes(&out["se"])
        } else {
            let img = gray_in(inp);
            let k = inp["k"].as_u64().map(|k| k as usize);
            let got = match id.split('-').next().unwrap() {
                "close" => close(&img, &Kernel::ellipse(k.unwrap(), k.unwrap())),
                "open" => open(&img, &Kernel::ellipse(k.unwrap(), k.unwrap())),
                "gradient" => gradient(&img, &Kernel::rect(3, 3)),
                "dilate" => dilate(&img, &Kernel::ellipse(5, 3)),
                _ => erode(&img, &Kernel::ellipse(5, 3)),
            };
            got.data == bytes(&out["out"])
        };
        if !ok {
            fails.push(format!("{id}: pixels differ"));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

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
