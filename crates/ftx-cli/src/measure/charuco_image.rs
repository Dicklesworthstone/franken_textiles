//! Optional external detection at the CLI boundary; Rust owns scale, refusals and uncertainty.
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_measure::camera::Intrinsics;
use ftx_measure::photo::{PhotoScale, analyze_reference};
use ftx_trace::raster::{Bgr, bgr_to_gray};
use serde_json::{Value, json};

use crate::{bad_input, read_json};

const DETECTOR: &str = include_str!("charuco_detector.py");
const MAX_OUTPUT: u64 = 256 * 1024;

fn detector_error(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::Unscaled, message)
        .repair("show one flat ChArUco board and use its original calib-sheet JSON sidecar")
        .repair("install opencv-python-headless==4.13.0.92 in the --detector-python environment")
}

fn read_bounded(reader: impl Read) -> std::io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader.take(MAX_OUTPUT + 1).read_to_end(&mut bytes)?;
    Ok(bytes)
}

fn detect(
    image: &Bgr,
    sheet: &Value,
    python: &str,
    intrinsics: Option<Intrinsics>,
) -> Result<Value, Refusal> {
    if let Some(k) = intrinsics {
        k.validate()?;
    }
    let mut header = serde_json::to_vec(&json!({
        "schema":"ftx-charuco-request/1", "image_size_px":[image.w,image.h], "sheet":sheet,
        "intrinsics":intrinsics
    }))
    .map_err(|e| bad_input(e.to_string()))?;
    header.push(b'\n');
    if header.len() > 65536 {
        return Err(bad_input(
            "ChArUco sidecar exceeds the 64 KiB detector header budget",
        ));
    }
    // No shell, filesystem image handoff or implicit dependency installation. -I excludes
    // cwd/PYTHONPATH imports; a moved standalone ftx binary retains its embedded adapter.
    let mut child = Command::new(python)
        .args(["-I", "-c", DETECTOR])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| detector_error(format!("cannot start ChArUco detector {python}: {e}")))?;
    let mut input = child.stdin.take().unwrap();
    let output = child.stdout.take().unwrap();
    let error = child.stderr.take().unwrap();
    let bytes = std::thread::scope(|scope| {
        // Drain both pipes while feeding pixels, including when a failed Python install exits
        // early. A timeout kills and reaps the child before joining the I/O workers.
        let writer = scope.spawn(move || {
            input.write_all(&header)?;
            input.write_all(&image.data)
        });
        let stdout = scope.spawn(move || read_bounded(output));
        let stderr = scope.spawn(move || read_bounded(error));
        let start = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Ok(status),
                Ok(None) if start.elapsed() < Duration::from_secs(30) => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                result => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break Err(match result {
                        Err(e) => detector_error(format!("ChArUco detector wait failed: {e}")),
                        _ => Refusal::new(
                            RefusalCode::BudgetExceeded,
                            "ChArUco detector exceeded 30 seconds; resize the oriented image and re-click",
                        ),
                    });
                }
            }
        };
        let written = writer
            .join()
            .map_err(|_| detector_error("detector input worker failed"))?;
        let out = stdout
            .join()
            .map_err(|_| detector_error("detector output worker failed"))?
            .map_err(|e| detector_error(e.to_string()))?;
        let err = stderr
            .join()
            .map_err(|_| detector_error("detector error worker failed"))?
            .map_err(|e| detector_error(e.to_string()))?;
        let status = status?;
        if out.len() as u64 > MAX_OUTPUT || err.len() as u64 > MAX_OUTPUT {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "ChArUco detector output exceeded 256 KiB",
            ));
        }
        if !status.success() {
            return Err(detector_error(format!(
                "ChArUco detector failed ({status}): {}",
                String::from_utf8_lossy(&err)
            )));
        }
        written.map_err(|e| detector_error(format!("ChArUco pixel transfer failed: {e}")))?;
        Ok(out)
    })?;
    let response: Value = serde_json::from_slice(&bytes)
        .map_err(|e| detector_error(format!("invalid ChArUco detector output: {e}")))?;
    if response["schema"] != "ftx-charuco-detection/1" {
        return Err(detector_error("unsupported ChArUco detector response"));
    }
    if let Some(error) = response.get("error") {
        return Err(detector_error(format!(
            "ChArUco detection refused: {error}"
        )));
    }
    Ok(response)
}

fn percentile(histogram: &[u32], fraction: f64, value: impl Fn(usize) -> f64) -> f64 {
    let count: u32 = histogram.iter().sum();
    let rank = fraction * f64::from(count - 1);
    let lo = rank.floor() as u32;
    let hi = rank.ceil() as u32;
    let mut seen = 0;
    let mut lower = None;
    for (i, &n) in histogram.iter().enumerate() {
        seen += n;
        if seen > lo && lower.is_none() {
            lower = Some(value(i));
        }
        if seen > hi {
            let a = lower.unwrap();
            return a + (rank - f64::from(lo)) * (value(i) - a);
        }
    }
    unreachable!("nonempty bounded pixel histogram")
}

// Calibration must retain every identified corner and its predeclared partition. Do not run
// photo-scale's RANSAC here: dropping a held-out residual would invalidate its independent check.
pub(super) fn calibration_corners(
    image: &Bgr,
    sheet: &Value,
    python: &str,
) -> Result<Value, Refusal> {
    let mut response = detect(image, sheet, python, None)?;
    response["adapter_sha256"] = json!(sha256_hex(DETECTOR.as_bytes()));
    Ok(response)
}

// Spec A.10 Sobel/percentile diagnostic. The padding comes from Rust's fitted scale, not
// an independent OpenCV homography. No texture/contrast means unknown, never zero blur.
fn blur(image: &Bgr, reference: &Value, square_px: f64) -> Option<f64> {
    let corners = reference["corners"].as_array()?;
    let mut low = [f64::INFINITY; 2];
    let mut high = [f64::NEG_INFINITY; 2];
    for corner in corners {
        for k in 0..2 {
            let v = corner["point_px"][k].as_f64()?;
            low[k] = low[k].min(v);
            high[k] = high[k].max(v);
        }
    }
    let x0 = (low[0] - square_px).floor().max(0.0) as usize;
    let y0 = (low[1] - square_px).floor().max(0.0) as usize;
    let x1 = (high[0] + square_px).ceil().min(image.w as f64) as usize;
    let y1 = (high[1] + square_px).ceil().min(image.h as f64) as usize;
    let w = x1.checked_sub(x0)?;
    let h = y1.checked_sub(y0)?;
    if w < 3 || h < 3 || w * h < 100 {
        return None;
    }
    let gray = bgr_to_gray(image);
    // Exact integer histograms avoid sorting millions of f64s. A 3x3 Sobel component
    // is in -1020..=1020; percentile interpolation is applied after sqrt, not before.
    let mut values = [0_u32; 256];
    let mut gradients = vec![0_u32; 2 * 1020 * 1020 + 1];
    // Reflect-101 at the ROI boundary, as OpenCV Sobel's default border mode.
    let reflect = |i: isize, n: usize| -> usize {
        if i < 0 {
            1
        } else if i >= n as isize {
            n - 2
        } else {
            i as usize
        }
    };
    let sample = |x: isize, y: isize| i32::from(gray.at(x0 + reflect(x, w), y0 + reflect(y, h)));
    for y in 0..h as isize {
        for x in 0..w as isize {
            values[sample(x, y) as usize] += 1;
            let gx = sample(x + 1, y - 1) + 2 * sample(x + 1, y) + sample(x + 1, y + 1)
                - sample(x - 1, y - 1)
                - 2 * sample(x - 1, y)
                - sample(x - 1, y + 1);
            let gy = sample(x - 1, y + 1) + 2 * sample(x, y + 1) + sample(x + 1, y + 1)
                - sample(x - 1, y - 1)
                - 2 * sample(x, y - 1)
                - sample(x + 1, y - 1);
            gradients[(gx * gx + gy * gy) as usize] += 1;
        }
    }
    let contrast =
        percentile(&values, 0.98, |i| i as f64) - percentile(&values, 0.02, |i| i as f64);
    let gradient = percentile(&gradients, 0.995, |i| (i as f64).sqrt() / 8.0);
    if contrast <= 0.0 || gradient <= 0.0 {
        return None;
    }
    let raw = contrast / ((2.0 * std::f64::consts::PI).sqrt() * gradient);
    Some((raw * raw - 0.64).max(0.0).sqrt())
}

pub(super) fn analyze(
    image: &Bgr,
    sheet_path: &str,
    python: Option<&str>,
    intrinsics: Option<Intrinsics>,
) -> Result<(PhotoScale, Value), Refusal> {
    let sheet = read_json(sheet_path)?.0;
    if sheet["schema"] != "fabric-calib-sheet/1" || sheet["board"]["type"] != "charuco" {
        return Err(bad_input(
            "--sheet must be a fabric-calib-sheet/1 ChArUco sidecar",
        ));
    }
    let response = match python {
        None | Some("rust") => {
            ftx_measure::photo::detect_charuco(&image.data, image.w, image.h, &sheet, intrinsics)?
        }
        Some(py) => detect(image, &sheet, py, intrinsics)?,
    };
    let mut reference = response["reference"].clone();
    if reference["reference"] != "charuco"
        || reference["image_size_px"] != json!([image.w, image.h])
    {
        return Err(detector_error(
            "ChArUco detector returned the wrong reference/image frame",
        ));
    }
    // Rebuild physical metadata from the supplied sidecar, never trust a worker to change scale.
    reference["board"] = json!({
        "squares_x":sheet["board"]["squares_x"], "squares_y":sheet["board"]["squares_y"],
        "square_mm":sheet["board"]["square_mm"],
        "origin_mm":sheet["board"].get("origin_mm").cloned().unwrap_or(json!([0,0]))
    });
    reference["paper"] = sheet["paper"].clone();
    if reference.get("intrinsics").is_some() {
        return Err(detector_error(
            "ChArUco detector cannot supply or override physical lens calibration",
        ));
    }
    if let Some(k) = intrinsics {
        reference["intrinsics"] = json!(k);
    }
    let mut scale = analyze_reference(&reference)?;
    if let Some(square_px) = scale.report()["square_px"].as_f64() {
        reference["blur_sigma_px_estimate"] = json!(blur(image, &reference, square_px));
        // Same deterministic inlier fit; only the image-derived diagnostic changes.
        scale = analyze_reference(&reference)?;
    }
    let receipt = json!({"sheet_path":sheet_path, "sheet":sheet,
        "adapter_sha256":sha256_hex(DETECTOR.as_bytes()), "implementation":response["detector"],
        "reference_input":reference, "blur_method":"Sobel3/8 ROI percentiles (spec A.10)"});
    Ok((scale, receipt))
}

pub(super) fn annotate(report: &mut Value) {
    if report["image_input"].get("charuco_detection").is_some() {
        report["coordinate_source"] = json!("detected_charuco");
        report["notes"][0] = json!(
            "ChArUco IDs and subpixel corners detected locally; homography, inliers and uncertainty computed in Rust. Pixels are as displayed: a PNG as stored, a JPEG with its EXIF orientation applied."
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn histogram_percentiles_interpolate_magnitudes_not_squared_values() {
        let mut histogram = vec![0; 101];
        histogram[0] = 1;
        histogram[4] = 1;
        histogram[100] = 2;
        assert_eq!(percentile(&histogram, 0.5, |i| (i as f64).sqrt()), 6.0);
        assert_eq!(percentile(&histogram, 0.0, |i| i as f64), 0.0);
        assert_eq!(percentile(&histogram, 1.0, |i| i as f64), 100.0);
    }

    #[test]
    fn blur_requires_texture_and_recognizes_an_unblurred_step() {
        let mut image = Bgr {
            w: 100,
            h: 100,
            data: vec![80; 100 * 100 * 3],
        };
        let reference = json!({"corners":[{"point_px":[10,10]},{"point_px":[90,90]}]});
        assert_eq!(blur(&image, &reference, 10.0), None);
        for y in 0..100 {
            for x in 50..100 {
                image.data[3 * (y * 100 + x)..3 * (y * 100 + x) + 3].fill(240);
            }
        }
        assert_eq!(blur(&image, &reference, 10.0), Some(0.0));
        assert_eq!(blur(&image, &json!({"corners":[]}), 10.0), None);
    }
}
