//! Optional external detection at the CLI boundary; Rust owns scale, refusals and uncertainty.
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_measure::camera::Intrinsics;
use ftx_measure::photo::{PhotoScale, acquisition::analyze_charuco_detection};
use ftx_trace::raster::Bgr;
use serde_json::{Value, json};

use crate::{bad_input, read_json};

pub(super) use ftx_measure::photo::acquisition::annotate_charuco as annotate;

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

// Calibration must retain every identified corner and its predeclared partition. Do not run
// photo-scale's RANSAC here: dropping a held-out residual would invalidate its independent check.
pub(super) fn calibration_corners(
    image: &Bgr,
    sheet: &Value,
    python: &str,
) -> Result<Value, Refusal> {
    if python == "rust" {
        return ftx_measure::photo::detect_charuco(&image.data, image.w, image.h, sheet, None);
    }
    let mut response = detect(image, sheet, python, None)?;
    response["adapter_sha256"] = json!(sha256_hex(DETECTOR.as_bytes()));
    Ok(response)
}

pub(super) fn analyze(
    image: &Bgr,
    sheet_path: &str,
    python: Option<&str>,
    intrinsics: Option<Intrinsics>,
) -> Result<(PhotoScale, Value), Refusal> {
    let (sheet, sheet_bytes) = read_json(sheet_path)?;
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
    let (scale, mut receipt) =
        analyze_charuco_detection(&image.data, image.w, image.h, &sheet, &response, intrinsics)?;
    receipt["sheet_path"] = json!(sheet_path);
    receipt["sheet_sha256"] = json!(sha256_hex(&sheet_bytes));
    receipt["adapter_sha256"] = json!(sha256_hex(DETECTOR.as_bytes()));
    Ok((scale, receipt))
}
