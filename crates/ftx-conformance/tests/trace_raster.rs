//! The trace's raster operations against `tests/fixtures/oracle/trace/raster_*.json` (captured by
//! `tools/oracle/capture_raster.py` from the reference's Pillow / OpenCV / numpy): every value bit for bit.
//! - `raster_image`: `load_image` on PNGs of every bit depth and colour type (all filters, Adam7, palette and
//!   gray/RGB transparency, partial alpha) → the BGR array.
//! - `raster_jpeg` (`tools/oracle/capture_jpeg.py`) and `raster_webp` (`tools/oracle/capture_webp.py`):
//!   `load_image` on JPEG and WebP files → the BGR array, or a refusal.
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

fn mask_png(v: &Value) -> Gray {
    let bgr = load_png(&base64(v.as_str().unwrap())).unwrap();
    Gray {
        w: bgr.w,
        h: bgr.h,
        data: bgr.data.iter().step_by(3).copied().collect(),
    }
}

/// `HoughLinesP` as the trace calls it (dash bridging on the sheet masks and a dashed edge, the grain
/// fallback on lines in noise): the same segments in the same order.
#[test]
fn hough_matches_opencv() {
    use ftx_trace::raster::hough::hough_lines_p;
    let fx = fixture("trace", "raster_hough");
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let (inp, id) = (&case["input"], case["id"].as_str().unwrap());
        let img = mask_png(&inp["mask_png"]);
        let got = hough_lines_p(
            &img,
            inp["rho"].as_f64().unwrap(),
            inp["theta"].as_f64().unwrap(),
            inp["threshold"].as_i64().unwrap() as i32,
            inp["min_len"].as_f64().unwrap(),
            inp["max_gap"].as_f64().unwrap(),
        );
        let want: Vec<[i32; 4]> = case["output"]["lines"]
            .as_array()
            .unwrap()
            .iter()
            .map(|l| [0, 1, 2, 3].map(|k| l[k].as_i64().unwrap() as i32))
            .collect();
        if got != want {
            let k = got
                .iter()
                .zip(&want)
                .position(|(a, b)| a != b)
                .unwrap_or(got.len().min(want.len()));
            fails.push(format!(
                "{id}: {} vs {} lines, first difference at {k}: {:?} vs {:?}",
                got.len(),
                want.len(),
                got.get(k),
                want.get(k)
            ));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// `fillPoly` (random, concave, self-intersecting and partly outside polygons; several at once),
/// `drawContours(FILLED)` on contours of blobs and the sheets, thick lines and `convexHull`, bit for bit.
#[test]
fn drawing_and_hull_match_opencv() {
    use ftx_trace::raster::contours::{approx_poly_dp, convex_hull};
    use ftx_trace::raster::draw::{fill_poly, line};
    let fx = fixture("trace", "raster_draw");
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let (inp, out, id) = (
            &case["input"],
            &case["output"],
            case["id"].as_str().unwrap(),
        );
        if id.starts_with("hull-") {
            let pts: Vec<(f32, f32)> = inp["pts"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32))
                .collect();
            let hull = convex_hull(&pts);
            let want: Vec<(f32, f32)> = out["hull"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_f64().unwrap() as f32, p[1].as_f64().unwrap() as f32))
                .collect();
            let xs = pts.iter().map(|p| p.0).fold(f32::NEG_INFINITY, f32::max)
                - pts.iter().map(|p| p.0).fold(f32::INFINITY, f32::min);
            let n = approx_poly_dp(&hull, 0.15 * f64::from(xs + 1.0), true).len();
            if hull != want || n as u64 != out["approx_n"].as_u64().unwrap() {
                fails.push(format!(
                    "{id}: hull {} vs {} points, approx {n} vs {}",
                    hull.len(),
                    want.len(),
                    out["approx_n"]
                ));
            }
            continue;
        }
        let (w, h) = (
            inp["w"].as_u64().unwrap() as usize,
            inp["h"].as_u64().unwrap() as usize,
        );
        let mut img = Gray::filled(w, h, 0);
        if let Some(polys) = inp.get("polys") {
            let polys: Vec<Vec<(i64, i64)>> = polys
                .as_array()
                .unwrap()
                .iter()
                .map(|q| {
                    q.as_array()
                        .unwrap()
                        .iter()
                        .map(|p| (p[0].as_i64().unwrap(), p[1].as_i64().unwrap()))
                        .collect()
                })
                .collect();
            fill_poly(&mut img, &polys, 255);
        } else {
            let p = |k: &str| (inp[k][0].as_i64().unwrap(), inp[k][1].as_i64().unwrap());
            line(
                &mut img,
                p("p0"),
                p("p1"),
                255,
                inp["thickness"].as_i64().unwrap(),
            );
        }
        let ok = match out.get("sha256") {
            Some(sha) => ftx_core::receipt::sha256_hex(&img.data) == sha.as_str().unwrap(),
            None => img.data == bytes(&out["img"]),
        };
        if !ok {
            fails.push(format!("{id}: pixels differ"));
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// `Polygon.buffer` as the trace calls it (stroke-centre mitre offsets and marks insets of the sheets'
/// outlines; synthetic stars, ells and slivers with mitre and round joins, both ways).
#[test]
fn polygon_buffers_match_geos() {
    use ftx_core::Vec2;
    use ftx_trace::buffer::{Join, Polygon, buffer};
    let fx = fixture("trace", "raster_buffer");
    let ring = |v: &Value| -> Vec<Vec2> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|p| Vec2::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
            .collect()
    };
    let area = |r: &[Vec2]| {
        r.windows(2)
            .map(|w| w[0].x * w[1].y - w[1].x * w[0].y)
            .sum::<f64>()
            .abs()
            / 2.0
    };
    let (mut exact, mut total) = (0, 0);
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let (inp, out, id) = (
            &case["input"],
            &case["output"],
            case["id"].as_str().unwrap(),
        );
        let join = if inp["join"] == "mitre" {
            Join::Mitre(4.0)
        } else {
            Join::Round(16)
        };
        let mut got: Vec<Vec<Vec2>> = buffer(
            &[Polygon::new(ring(&inp["ring"]))],
            inp["distance"].as_f64().unwrap(),
            join,
        )
        .unwrap()
        .into_iter()
        .map(|p| p.shell)
        .collect();
        got.sort_by(|a, b| area(b).total_cmp(&area(a)));
        let mut want: Vec<Vec<Vec2>> = out["polygons"]
            .as_array()
            .unwrap()
            .iter()
            .map(&ring)
            .collect();
        want.sort_by(|a, b| area(b).total_cmp(&area(a)));
        total += 1;
        if got.len() != want.len() {
            fails.push(format!("{id}: {} polygons vs {}", got.len(), want.len()));
            continue;
        }
        if let (Some(g), Some(w)) = (got.first(), want.first()) {
            if g == w {
                exact += 1;
                continue;
            }
            // the stroke-centre offsets set the trace's resampling phase: identical rings required
            if id.starts_with("stroke-") {
                fails.push(format!(
                    "{id}: not identical ({} vs {} points)",
                    g.len(),
                    w.len()
                ));
            }
            let dev = g
                .iter()
                .map(|p| w.iter().map(|q| p.dist(*q)).fold(f64::INFINITY, f64::min))
                .fold(0.0, f64::max);
            let rel_area = (area(g) / area(w) - 1.0).abs();
            println!(
                "{id}: {} vs {} points, start {:?} vs {:?}, max deviation {dev:.2e}, area {rel_area:.1e}",
                g.len(),
                w.len(),
                g[0],
                w[0]
            );
            if dev > 0.05 || rel_area > 1e-3 {
                fails.push(format!("{id}: deviation {dev:.3} px, area {rel_area:.2e}"));
            }
        }
    }
    println!("{exact} of {total} buffers identical");
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}

/// Chamfer distance transforms (masks 3 and 5) and 8-connected components with their stats, bit for bit.
#[test]
fn distance_and_components_match_opencv() {
    use ftx_trace::raster::distance::{components8, distance_l2};
    let fx = fixture("trace", "raster_distance");
    let mut fails = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let (inp, out) = (&case["input"], &case["output"]);
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
        let stats_of = |labels: &[u32],
                        stats: &[ftx_trace::raster::distance::Stats]|
         -> (Vec<Vec<i64>>, Vec<u32>) {
            (
                stats
                    .iter()
                    .map(|s| {
                        vec![
                            s.x as i64,
                            s.y as i64,
                            s.w as i64,
                            s.h as i64,
                            s.area as i64,
                        ]
                    })
                    .collect(),
                labels.to_vec(),
            )
        };
        let want_stats = |v: &Value| -> Vec<Vec<i64>> {
            v.as_array()
                .unwrap()
                .iter()
                .map(|r| {
                    r.as_array()
                        .unwrap()
                        .iter()
                        .map(|x| x.as_i64().unwrap())
                        .collect()
                })
                .collect()
        };
        if id.starts_with("sheet-") {
            for m in [3u8, 5] {
                let dt = distance_l2(&img, m);
                let bytes: Vec<u8> = dt.data.iter().flat_map(|v| v.to_le_bytes()).collect();
                if ftx_core::receipt::sha256_hex(&bytes) != out[format!("dt{m}")].as_str().unwrap()
                {
                    fails.push(format!("{id}: dt{m} differs"));
                }
            }
            let (labels, stats) = components8(&img);
            let (s, l) = stats_of(&labels, &stats);
            let lbytes: Vec<u8> = l.iter().flat_map(|v| v.to_le_bytes()).collect();
            if s != want_stats(&out["stats"])
                || ftx_core::receipt::sha256_hex(&lbytes) != out["labels_sha256"].as_str().unwrap()
            {
                fails.push(format!(
                    "{id}: components differ ({} vs {})",
                    stats.len(),
                    out["n"]
                ));
            }
        } else if let Some(m) = inp.get("mask").and_then(Value::as_u64) {
            let dt = distance_l2(&img, m as u8);
            let want: Vec<f32> = out["dt"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap() as f32)
                .collect();
            if dt.data != want {
                let k = dt.data.iter().zip(&want).position(|(a, b)| a != b).unwrap();
                fails.push(format!("{id}: pixel {k}: {} vs {}", dt.data[k], want[k]));
            }
        } else {
            let (labels, stats) = components8(&img);
            let (s, l) = stats_of(&labels, &stats);
            let want_l: Vec<u32> = out["labels"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect();
            if s != want_stats(&out["stats"]) || l != want_l {
                fails.push(format!(
                    "{id}: components differ ({} vs {})",
                    stats.len(),
                    out["n"]
                ));
            }
        }
    }
    assert!(fails.is_empty(), "{}", fails.join("\n"));
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

/// `load_image` on JPEGs (`raster_jpeg`, captured by `tools/oracle/capture_jpeg.py`): every sampling factor
/// and colour space both encoders write, progressive and restart files, odd sizes, extreme tables and damaged
/// streams. Each decodes to the reference's BGR bit for bit (its SHA-256 for larger images), or both refuse.
#[test]
fn jpeg_loading_matches_pillow() {
    image_loading_matches_pillow("raster_jpeg", "jpeg");
}

/// `load_image` on WebPs (`raster_webp`, `tools/oracle/capture_webp.py`): lossy and lossless, alpha
/// composited on white, every Pillow method, cwebp's filters and segments, libvpx's token partitions, the
/// container's variants and damaged files; bit for bit, or both refuse.
#[test]
fn webp_loading_matches_pillow() {
    image_loading_matches_pillow("raster_webp", "webp");
}

fn image_loading_matches_pillow(name: &str, key: &str) {
    let fx = fixture("trace", name);
    let (mut fails, mut decoded, mut refused) = (Vec::new(), 0, 0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let out = &case["output"];
        let got = ftx_trace::raster::load_image(&base64(case["input"][key].as_str().unwrap()));
        match (got, out.get("error")) {
            (Err(_), Some(_)) => refused += 1,
            (Ok(_), Some(e)) => fails.push(format!("{id}: decoded, the reference refused ({e})")),
            (Err(e), None) => fails.push(format!("{id}: refused: {e}")),
            (Ok(img), None) => {
                let shape: Vec<u64> = out["shape"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap())
                    .collect();
                let same = if shape != [img.h as u64, img.w as u64, 3] {
                    false
                } else if let Some(sha) = out.get("sha256") {
                    ftx_core::receipt::sha256_hex(&img.data) == sha.as_str().unwrap()
                } else {
                    img.data == bytes(&out["bgr"])
                };
                if same {
                    decoded += 1;
                } else {
                    fails.push(format!(
                        "{id}: {}×{} differs from the reference",
                        img.w, img.h
                    ));
                }
            }
        }
    }
    println!("{name}: {decoded} decoded bit for bit, {refused} refused as the reference refuses");
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

/// `buffer` where GEOS's topology decides (`buffer_topology`, `tools/oracle/capture_buffer_topology.py`):
/// self-crossing offset curves (where the ring starts), splitting and vanishing insets, the distance-0
/// clean-up of spikes, pinches and bowties, holes, multipolygons, traced stroke outlines. The polygons, their
/// order, every ring's start and length and the holes are GEOS's; coordinates are bit-identical except where a
/// join's `atan2`/`sin`/`cos` reach the last bits (1e-9 relative). Where GEOS needed its reduced-precision
/// retry, the buffer refuses as GEOS's first attempt did.
#[test]
fn buffer_topology_matches_geos() {
    use ftx_core::Vec2;
    use ftx_trace::buffer::{Join, Polygon, buffer};
    let fx = fixture("trace", "buffer_topology");
    let ring = |v: &Value| -> Vec<Vec2> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|p| Vec2::new(p[0].as_f64().unwrap(), p[1].as_f64().unwrap()))
            .collect()
    };
    let poly = |v: &Value| Polygon {
        shell: ring(&v["shell"]),
        holes: v["holes"]
            .as_array()
            .map(|h| h.iter().map(&ring).collect())
            .unwrap_or_default(),
    };
    let close = |a: &[Vec2], b: &[Vec2]| {
        a.len() == b.len()
            && a.iter().zip(b).all(|(p, q)| {
                (p.x - q.x).abs() <= 1e-9 * q.x.abs().max(1.0)
                    && (p.y - q.y).abs() <= 1e-9 * q.y.abs().max(1.0)
            })
    };
    let (mut exact, mut fails) = (0, Vec::new());
    let cases = fx["cases"].as_array().unwrap();
    for case in cases {
        let (inp, out, id) = (
            &case["input"],
            &case["output"],
            case["id"].as_str().unwrap(),
        );
        let polys: Vec<Polygon> = inp["polys"].as_array().unwrap().iter().map(poly).collect();
        let join = if inp["join"] == "mitre" {
            Join::Mitre(4.0)
        } else {
            Join::Round(16)
        };
        let got = buffer(&polys, inp["distance"].as_f64().unwrap(), join);
        if out["reduced_precision"] == true {
            if got.is_ok() {
                fails.push(format!(
                    "{id}: GEOS needed its reduced-precision retry; this did not refuse"
                ));
            }
            continue;
        }
        let want: Vec<Polygon> = out["polygons"]
            .as_array()
            .unwrap()
            .iter()
            .map(poly)
            .collect();
        match got {
            Err(e) => fails.push(format!("{id}: refused ({e})")),
            Ok(got) if got == want => exact += 1,
            Ok(got) => {
                let same = got.len() == want.len()
                    && got.iter().zip(&want).all(|(g, w)| {
                        close(&g.shell, &w.shell)
                            && g.holes.len() == w.holes.len()
                            && g.holes.iter().zip(&w.holes).all(|(a, b)| close(a, b))
                    });
                if !same {
                    fails.push(format!(
                        "{id}: {} polygons ({:?} points) vs {} ({:?})",
                        got.len(),
                        got.iter().map(|p| p.shell.len()).collect::<Vec<_>>(),
                        want.len(),
                        want.iter().map(|p| p.shell.len()).collect::<Vec<_>>()
                    ));
                }
            }
        }
    }
    println!("{exact} of {} buffers bit-identical", cases.len());
    assert!(fails.is_empty(), "{}", fails.join("\n"));
}
