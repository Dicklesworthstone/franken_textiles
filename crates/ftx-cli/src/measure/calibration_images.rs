//! ChArUco photographs -> shared Rust camera calibration, with predeclared held-out evidence.
//! Only detection is external. Pixels are processed once per view and never embedded in a report.

use std::{collections::BTreeSet, path::Path};

use ftx_core::{Refusal, RefusalCode, Vec2, receipt::sha256_hex};
use ftx_measure::{
    photo::{
        MAX_PHOTO_PIXELS,
        intrinsics::{CalibrationInput, Control, Corner, DistortionFit, View},
    },
    triangulate::{FrameId, Partition},
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{charuco_image, load_image};
use crate::{bad_input, measure::intrinsics::read_document};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    image: String,
    frame: FrameId,
    partition: Partition,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    image_model: String,
    lens_id: String,
    #[serde(default)]
    pinhole_evidence: String,
    #[serde(default)]
    distortion_fit: Option<DistortionFit>,
    reference_evidence: String,
    image_size_px: [u32; 2],
    rotation_cw_deg: u16,
    held_out_corner_ids: Vec<usize>,
    views: Vec<Capture>,
}

struct Board {
    columns: usize,
    rows: usize,
    square_m: f64,
}

fn board(sheet: &Value) -> Result<Board, Refusal> {
    let b = &sheet["board"];
    let count = |name| {
        b[name]
            .as_u64()
            .filter(|n| (3..=64).contains(n))
            .map(|n| n as usize)
            .ok_or_else(|| bad_input(format!("board {name} must be an integer in 3..=64")))
    };
    if sheet["schema"] != "fabric-calib-sheet/1"
        || b["type"] != "charuco"
        || b["dictionary"] != "DICT_4X4_50"
        || !matches!(sheet["paper"].as_str(), Some("letter" | "a4"))
    {
        return Err(bad_input(
            "--sheet requires a DICT_4X4_50 fabric-calib-sheet/1 sidecar",
        ));
    }
    let (sx, sy) = (count("squares_x")?, count("squares_y")?);
    let square = b["square_mm"].as_f64().unwrap_or(f64::NAN);
    let marker = b["marker_mm"].as_f64().unwrap_or(f64::NAN);
    if !square.is_finite()
        || !marker.is_finite()
        || !(0.0 < marker && marker < square && square <= 1000.0)
        || sx * sy / 2 > 50
        || (sx - 1) * (sy - 1) > 128
    {
        return Err(bad_input(
            "board must fit its dictionary and 128-control budget, with 0 < marker_mm < square_mm <= 1000",
        ));
    }
    // Full sidecar validation remains in the shared detector, including legacy layout and size_mm.
    Ok(Board {
        columns: sx - 1,
        rows: sy - 1,
        square_m: square / 1000.0,
    })
}

fn text(s: &str, limit: usize) -> bool {
    !s.trim().is_empty() && s.len() <= limit && !s.contains('\0')
}

fn prepare(manifest: &Manifest, board: &Board) -> Result<CalibrationInput, Refusal> {
    match (manifest.image_model.as_str(), &manifest.distortion_fit) {
        ("pinhole_zero_skew", None) if text(&manifest.pinhole_evidence, 4096) => {}
        ("brown5_zero_skew", Some(fit)) if manifest.pinhole_evidence.is_empty() => {
            fit.validate()?;
        }
        _ => {
            return Err(bad_input(
                "select pinhole_zero_skew with pinhole_evidence, or brown5_zero_skew with distortion_fit; never mix raw and rectified models",
            ));
        }
    }
    let pixels = u64::from(manifest.image_size_px[0]) * u64::from(manifest.image_size_px[1]);
    if manifest.views.len() > 32
        || pixels > MAX_PHOTO_PIXELS as u64
        || pixels.saturating_mul(manifest.views.len() as u64) > 128_000_000
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "image calibration permits 32 views, 16 million pixels per image and 128 million pixels total",
        ));
    }
    if manifest.schema != "ftx-camera-calibration-images/1"
        || !text(&manifest.lens_id, 256)
        || !text(&manifest.reference_evidence, 4096)
        || manifest.image_size_px.iter().any(|n| *n < 16)
        || ![0, 90, 180, 270].contains(&manifest.rotation_cw_deg)
    {
        return Err(bad_input(
            "image calibration needs schema /1, explicit pinhole/lens/reference evidence, dimensions and already-applied rotation",
        ));
    }
    let fits = manifest
        .views
        .iter()
        .filter(|v| v.partition == Partition::Fit)
        .count();
    let minimum_views = if manifest.distortion_fit.is_some() {
        4
    } else {
        3
    };
    if fits < minimum_views || fits == manifest.views.len() {
        return Err(bad_input(format!(
            "image calibration requires at least {minimum_views} fit views and one independently held-out view, assigned before fitting"
        )));
    }
    let total = board.columns * board.rows;
    let held: BTreeSet<_> = manifest.held_out_corner_ids.iter().copied().collect();
    if held.len() != manifest.held_out_corner_ids.len()
        || held.len() < 4
        || held.iter().any(|&id| id >= total)
        || total - held.len() < minimum_fit_corners(manifest)
    {
        return Err(bad_input(format!(
            "declare at least four unique in-range held_out_corner_ids and leave at least {} fit corners",
            minimum_fit_corners(manifest)
        )));
    }
    let mut frames = BTreeSet::new();
    let mut paths = BTreeSet::new();
    for view in &manifest.views {
        if !text(&view.image, 4096)
            || !paths.insert(&view.image)
            || !text(&view.frame.stream, 256)
            || view.frame.stream != manifest.views[0].frame.stream
            || !frames.insert(&view.frame)
        {
            return Err(bad_input(
                "calibration views require distinct image paths and decoded identities in one unchanged lens stream",
            ));
        }
    }
    let controls = (0..total)
        .map(|id| Control {
            id: id.to_string(),
            // Board origin is immaterial to intrinsics. This is the board's local x/y plane,
            // represented as sheet-local x/z in the camera API, in metres, not page millimetres.
            sheet_xz_m: Vec2::new(
                (id % board.columns + 1) as f64 * board.square_m,
                (id / board.columns + 1) as f64 * board.square_m,
            ),
        })
        .collect();
    Ok(CalibrationInput {
        schema: "ftx-camera-calibration-input/1".into(),
        image_model: manifest.image_model.clone(),
        lens_id: manifest.lens_id.clone(),
        pinhole_evidence: manifest.pinhole_evidence.clone(),
        distortion_fit: manifest.distortion_fit.clone(),
        reference_evidence: manifest.reference_evidence.clone(),
        image_size_px: manifest.image_size_px,
        rotation_cw_deg: manifest.rotation_cw_deg,
        controls,
        views: Vec::new(),
    })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Detection {
    id: usize,
    point_px: Vec2,
}

fn minimum_fit_corners(manifest: &Manifest) -> usize {
    if manifest.distortion_fit.is_some() {
        12
    } else {
        8
    }
}

fn observations(
    response: &Value,
    manifest: &Manifest,
    total: usize,
) -> Result<Vec<Corner>, Refusal> {
    let reference = &response["reference"];
    if reference["schema"] != "ftx-photo-reference/1"
        || reference["reference"] != "charuco"
        || reference["image_size_px"] != json!(manifest.image_size_px)
        || reference.get("intrinsics").is_some()
    {
        return Err(bad_input(
            "detector must return original ChArUco image coordinates, without fitted intrinsics",
        ));
    }
    if !reference["markers_detected"]
        .as_u64()
        .is_some_and(|n| (2..=50).contains(&n))
    {
        return Err(Refusal::new(
            RefusalCode::Unscaled,
            "calibration needs at least two identified markers on the selected board",
        ));
    }
    let detected: Vec<Detection> = serde_json::from_value(reference["corners"].clone())
        .map_err(|e| bad_input(format!("invalid calibration detections: {e}")))?;
    if detected.len() > total {
        return Err(bad_input(
            "more detected corners than the selected board contains",
        ));
    }
    let mut seen = BTreeSet::new();
    let mut points = Vec::new();
    for p in detected {
        if p.id >= total
            || !seen.insert(p.id)
            || !p.point_px.is_finite()
            || p.point_px.x < 0.0
            || p.point_px.y < 0.0
            || p.point_px.x > f64::from(manifest.image_size_px[0] - 1)
            || p.point_px.y > f64::from(manifest.image_size_px[1] - 1)
        {
            return Err(bad_input(
                "calibration corner IDs must be unique and on the board, with pixels inside the declared image",
            ));
        }
        points.push(Corner {
            id: p.id.to_string(),
            pixel_px: p.point_px,
            partition: if manifest.held_out_corner_ids.contains(&p.id) {
                Partition::HeldOut
            } else {
                Partition::Fit
            },
        });
    }
    let fit = points
        .iter()
        .filter(|p| p.partition == Partition::Fit)
        .count();
    let held = points.len() - fit;
    let minimum = minimum_fit_corners(manifest);
    if fit < minimum || held < 4 {
        return Err(Refusal::new(RefusalCode::Unscaled,
            format!("insufficient ChArUco evidence: {fit} fit and {held} held-out corners; need at least {minimum} and 4 in EVERY image"))
            .repair("retake a sharp, flat, well-lit board with the predeclared held-out corners visible; do not change partitions to rescue a fit"));
    }
    points.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(points)
}

pub(in crate::measure) fn load(
    manifest_path: &str,
    sheet_path: &str,
    python: &str,
) -> Result<(CalibrationInput, Value), Refusal> {
    let (manifest_value, manifest_bytes) = read_document(manifest_path)?;
    let manifest: Manifest = serde_json::from_value(manifest_value.clone())
        .map_err(|e| bad_input(format!("invalid image calibration manifest: {e}")))?;
    let (sheet, sheet_bytes) = read_document(sheet_path)?;
    let board = board(&sheet)?;
    let mut input = prepare(&manifest, &board)?;
    let base = Path::new(manifest_path).parent().unwrap_or(Path::new("."));
    let mut captures: Vec<_> = manifest.views.iter().collect();
    captures.sort_by(|a, b| a.frame.cmp(&b.frame));
    let mut images = BTreeSet::new();
    let mut receipts = Vec::new();
    for view in captures {
        let path = base.join(&view.image);
        let path = path
            .to_str()
            .ok_or_else(|| bad_input("image path must be UTF-8"))?;
        let (image, mut receipt) = load_image(path, MAX_PHOTO_PIXELS)?;
        if [image.w as u32, image.h as u32] != manifest.image_size_px {
            return Err(bad_input(format!(
                "{}: image_size_px differs from the calibration; no crop/resizing/rotation is inferred",
                view.image
            )));
        }
        // Encoded-byte hashes alone miss renamed/re-encoded copies crossing fit/held-out sets.
        let pixel_hash = sha256_hex(&image.data);
        if !images.insert(pixel_hash.clone()) {
            return Err(bad_input(
                "duplicate decoded calibration image: a renamed or re-encoded photograph is not an independent view",
            ));
        }
        let detected = charuco_image::calibration_corners(&image, &sheet, python)?;
        let corners =
            observations(&detected, &manifest, input.controls.len()).map_err(|mut e| {
                e.message = format!("{}: {}", view.image, e.message);
                e
            })?;
        receipt["pixels_bgr_sha256"] = json!(pixel_hash);
        receipt["frame"] = json!(view.frame);
        receipt["partition"] = json!(view.partition);
        receipt["corners_detected"] = json!(corners.len());
        receipt["held_out_corners"] = json!(
            corners
                .iter()
                .filter(|p| p.partition == Partition::HeldOut)
                .count()
        );
        receipt["detector"] = detected["detector"].clone();
        receipt["adapter_sha256"] = detected["adapter_sha256"].clone();
        receipts.push(receipt);
        input.views.push(View {
            frame: view.frame.clone(),
            partition: view.partition,
            corners,
        });
    }
    let receipt = json!({"schema":"ftx-camera-image-acquisition/1",
    "manifest_path":manifest_path,"manifest_sha256":sha256_hex(&manifest_bytes),"manifest":manifest_value,
    "sheet_path":sheet_path,"sheet_sha256":sha256_hex(&sheet_bytes),"sheet":sheet,"views":receipts,
    "corner_selection":"all detected corners; partitions declared before calibration; no RANSAC, trimming or fit-dependent redetection",
    "no_claim":if manifest.distortion_fit.is_some() {
        "Raw Brown-model pixels; no fitted-lens redetection. Inverse certification is not empirical lens-disk coverage. No intrinsic covariance, physical print-scale verification, or proof of unchanged lens settings. Detection errors may be correlated within a frame."
    } else {
        "Original pinhole pixels are required. No distortion fit, intrinsic covariance, physical print-scale verification, or proof of unchanged lens settings. Detection errors may be correlated within a frame."
    }});
    Ok((input, receipt))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest {
            schema: "ftx-camera-calibration-images/1".into(),
            image_model: "pinhole_zero_skew".into(),
            lens_id: "synthetic".into(),
            pinhole_evidence: "Analytical pinhole renderer".into(),
            distortion_fit: None,
            reference_evidence: "Synthetic, perfectly flat metric board".into(),
            image_size_px: [800, 600],
            rotation_cw_deg: 0,
            held_out_corner_ids: vec![0, 6, 28, 34],
            views: (0..4)
                .map(|i| Capture {
                    image: format!("frame{i}.png"),
                    frame: FrameId {
                        stream: "synthetic".into(),
                        timestamp_us: i * 100_000,
                    },
                    partition: if i == 3 {
                        Partition::HeldOut
                    } else {
                        Partition::Fit
                    },
                })
                .collect(),
        }
    }

    fn board() -> Board {
        Board {
            columns: 7,
            rows: 5,
            square_m: 0.022,
        }
    }

    fn detections() -> Value {
        json!({"reference":{"schema":"ftx-photo-reference/1","reference":"charuco",
            "image_size_px":[800,600],"markers_detected":24,
            "corners":(0..35).map(|i| json!({"id":i,"point_px":[50 + (i%7)*20,60+(i/7)*20]})).collect::<Vec<_>>()}})
    }

    #[test]
    fn physical_ids_and_predeclared_partitions_do_not_depend_on_detection_order() {
        let m = manifest();
        let input = prepare(&m, &board()).unwrap();
        assert_eq!(input.controls.len(), 35);
        assert_eq!(input.controls[0].sheet_xz_m, Vec2::new(0.022, 0.022));
        assert!((input.controls[34].sheet_xz_m.x - 0.154).abs() < 1e-12);
        assert!(input.views.is_empty());
        let mut r = detections();
        let a = observations(&r, &m, 35).unwrap();
        r["reference"]["corners"].as_array_mut().unwrap().reverse();
        assert_eq!(
            serde_json::to_value(&a).unwrap(),
            serde_json::to_value(observations(&r, &m, 35).unwrap()).unwrap()
        );
        for p in a {
            assert_eq!(
                p.partition == Partition::HeldOut,
                m.held_out_corner_ids.contains(&p.id.parse().unwrap())
            );
        }
        // Pixel residuals are not a selection criterion. The native solver must see and reject
        // an inconsistent observation rather than the acquisition silently trimming it.
        r["reference"]["corners"][0]["point_px"] = json!([780, 580]);
        assert_eq!(observations(&r, &m, 35).unwrap().len(), 35);
    }

    #[test]
    fn manifests_require_independent_views_and_predeclared_corner_holdouts() {
        for kind in 0..9 {
            let mut m = manifest();
            match kind {
                0 => m.views[3].partition = Partition::Fit,
                1 => m.views[0].partition = Partition::HeldOut,
                2 => m.views[1].frame = m.views[0].frame.clone(),
                3 => m.views[1].image = m.views[0].image.clone(),
                4 => m.views[1].frame.stream = "another lens".into(),
                5 => m.held_out_corner_ids = vec![0, 0, 6, 28, 34],
                6 => m.held_out_corner_ids = vec![0, 6, 28, 35],
                7 => m.rotation_cw_deg = 45,
                _ => m.pinhole_evidence.clear(),
            }
            assert!(prepare(&m, &board()).is_err(), "case {kind}");
        }
        let mut m = manifest();
        m.image_size_px = [4000, 4000];
        for i in 4..9 {
            m.views.push(Capture {
                image: format!("p{i}.png"),
                frame: FrameId {
                    stream: "synthetic".into(),
                    timestamp_us: i,
                },
                partition: Partition::Fit,
            });
        }
        assert_eq!(
            prepare(&m, &board()).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
    }

    #[test]
    fn insufficient_or_malformed_detections_never_become_calibration_views() {
        for kind in 0..7 {
            let mut r = detections();
            match kind {
                0 => r["reference"]["corners"]
                    .as_array_mut()
                    .unwrap()
                    .retain(|p| p["id"] != 0),
                1 => r["reference"]["corners"][1]["id"] = json!(0),
                2 => r["reference"]["corners"][1]["id"] = json!(35),
                3 => r["reference"]["corners"][1]["point_px"] = json!([-1, 50]),
                4 => r["reference"]["intrinsics"] = Value::Null,
                5 => r["reference"]["image_size_px"] = json!([1600, 1200]),
                _ => r["reference"]["markers_detected"] = json!(1),
            }
            assert!(observations(&r, &manifest(), 35).is_err(), "case {kind}");
        }
    }

    #[test]
    fn brown_acquisition_requires_explicit_raw_model_and_stronger_fit_evidence() {
        let mut m = manifest();
        m.image_model = "brown5_zero_skew".into();
        m.pinhole_evidence.clear();
        m.distortion_fit = Some(DistortionFit {
            model_evidence: "Independent synthetic Brown renderer".into(),
            max_radius: 0.9,
            max_iterations: 80,
        });
        assert!(prepare(&m, &board()).is_err()); // only three fit views
        m.views.push(Capture {
            image: "view4.png".into(),
            frame: FrameId {
                stream: "synthetic".into(),
                timestamp_us: 400_000,
            },
            partition: Partition::Fit,
        });
        let input = prepare(&m, &board()).unwrap();
        assert!(input.distortion_fit.is_some());
        assert!(input.pinhole_evidence.is_empty());
        let mut sparse = detections();
        sparse["reference"]["corners"]
            .as_array_mut()
            .unwrap()
            .retain(|p| p["id"].as_u64().unwrap() < 12 || p["id"] == 28 || p["id"] == 34);
        assert!(observations(&sparse, &m, 35).is_err()); // ten fit points, four holdouts
        m.pinhole_evidence = "Contradictory rectified-image claim".into();
        assert!(prepare(&m, &board()).is_err());
        m.pinhole_evidence.clear();
        m.distortion_fit.as_mut().unwrap().max_radius = -1.0;
        assert!(prepare(&m, &board()).is_err());
    }
}
