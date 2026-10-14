//! Imported ChArUco detections. IDs define physical corner locations; an explicit bounded RANSAC
//! rejects mismatches before measurement. This module never runs a marker or image detector.

use ftx_core::{Refusal, RefusalCode, det, py};
use serde_json::{Value, json};

use super::geometry::{
    Homography, Point, distort_point, fit, plane_scale, plane_scale_in_image, undistort_point,
};
use super::measurement::Random;
use super::{
    Calibration, PhotoScale, Samples, invalid, known, number, point, positive, scale_fields,
    scale_warnings,
};

fn count(v: &Value, label: &str, lo: u64, hi: u64) -> Result<usize, Refusal> {
    v.as_u64()
        .filter(|x| (lo..=hi).contains(x))
        .map(|x| x as usize)
        .ok_or_else(|| invalid(format!("{label} must be an integer in {lo}..={hi}")))
}

fn residual(h: &Homography, p: Point, q: Point) -> f64 {
    h.map(p)
        .map_or(f64::INFINITY, |v| det::hypot(v[0] - q[0], v[1] - q[1]))
}

fn inliers(h: &Homography, p: &[Point], q: &[Point]) -> Vec<usize> {
    p.iter()
        .zip(q)
        .enumerate()
        .filter_map(|(i, (&p, &q))| (residual(h, p, q) <= 2.0).then_some(i))
        .collect()
}

fn refit(p: &[Point], q: &[Point], ids: &[usize]) -> Result<Homography, Refusal> {
    fit(
        &ids.iter().map(|&i| p[i]).collect::<Vec<_>>(),
        &ids.iter().map(|&i| q[i]).collect::<Vec<_>>(),
    )
}

fn robust(p: &[Point], q: &[Point]) -> Result<(Homography, Vec<usize>), Refusal> {
    let mut rng = Random { state: 0 };
    let mut best = Vec::new();
    let mut best_error = f64::INFINITY;
    for _ in 0..512 {
        // Sample without replacement; fixed input ID order, seed and tie handling.
        let mut pool: Vec<usize> = (0..p.len()).collect();
        for i in 0..4 {
            let j = i + (rng.uniform() * (pool.len() - i) as f64) as usize;
            pool.swap(i, j);
        }
        let Ok(h) = refit(p, q, &pool[..4]) else {
            continue;
        };
        let ids = inliers(&h, p, q);
        let error: f64 = ids.iter().map(|&i| residual(&h, p[i], q[i]).powi(2)).sum();
        if ids.len() > best.len() || (ids.len() == best.len() && error < best_error) {
            best = ids;
            best_error = error;
        }
        if best.len() == p.len() {
            break;
        }
    }
    if best.len() < 6 {
        return Err(invalid(
            "homography fit failed: fewer than six consistent ChArUco corners",
        ));
    }
    let mut h = refit(p, q, &best)?;
    for _ in 0..4 {
        let next = inliers(&h, p, q);
        if next.len() < 6 {
            return Err(invalid("homography refit lost its six-corner consensus"));
        }
        if next == best {
            return Ok((h, best));
        }
        best = next;
        h = refit(p, q, &best)?;
    }
    Err(Refusal::new(
        RefusalCode::NotConverged,
        "ChArUco inlier selection did not stabilize",
    ))
}

pub(super) fn analyze(v: &Value) -> Result<PhotoScale, Refusal> {
    known(
        v,
        &[
            "schema",
            "reference",
            "image_size_px",
            "image",
            "paper",
            "board",
            "corners",
            "markers_detected",
            "blur_sigma_px_estimate",
            "intrinsics",
        ],
    )?;
    if v["schema"] != "ftx-photo-reference/1" {
        return Err(invalid("expected ftx-photo-reference/1"));
    }
    let intrinsics: Option<crate::camera::Intrinsics> = v
        .get("intrinsics")
        .map(|k| {
            serde_json::from_value(k.clone())
                .map_err(|e| invalid(format!("invalid photo intrinsics: {e}")))
        })
        .transpose()?;
    if let Some(k) = &intrinsics {
        k.validate()?;
    }
    for key in ["image", "paper"] {
        if v.get(key).is_some_and(|x| !x.is_string()) {
            return Err(invalid(format!("{key} must be a string")));
        }
    }
    let image = point(&v["image_size_px"], "image_size_px")?;
    if image
        .iter()
        .any(|v| *v < 2.0 || *v > 1_000_000.0 || v.fract() != 0.0)
    {
        return Err(invalid(
            "image_size_px must contain integer dimensions in 2..=1000000",
        ));
    }
    let board = &v["board"];
    known(board, &["squares_x", "squares_y", "square_mm", "origin_mm"])?;
    let sx = count(&board["squares_x"], "squares_x", 3, 64)?;
    let sy = count(&board["squares_y"], "squares_y", 3, 64)?;
    let square = positive(&board["square_mm"], "square_mm")?;
    if square > 1000.0 {
        return Err(invalid("square_mm exceeds 1000 mm"));
    }
    let origin = board
        .get("origin_mm")
        .map(|p| point(p, "origin_mm"))
        .transpose()?
        .unwrap_or([0.0; 2]);
    let size = [sx as f64 * square, sy as f64 * square];
    let total = (sx - 1) * (sy - 1);
    let corners = v["corners"]
        .as_array()
        .ok_or_else(|| invalid("corners must be a list of {id, point_px}"))?;
    if corners.len() > 512 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "at most 512 detected ChArUco corners",
        ));
    }
    let mut detections = Vec::new();
    for corner in corners {
        known(corner, &["id", "point_px"])?;
        let id = count(&corner["id"], "corner id", 0, total as u64 - 1)?;
        let p = point(&corner["point_px"], "point_px")?;
        if (0..2).any(|k| p[k] < 0.0 || p[k] > image[k] - 1.0) {
            return Err(invalid("detected corner outside oriented image"));
        }
        detections.push((id, p));
    }
    detections.sort_by_key(|(id, _)| *id);
    if detections.windows(2).any(|w| w[0].0 == w[1].0) {
        return Err(invalid("duplicate ChArUco corner id"));
    }
    let markers = v
        .get("markers_detected")
        .map(|x| count(x, "markers_detected", 0, 4096))
        .transpose()?;
    let blur = v
        .get("blur_sigma_px_estimate")
        .filter(|x| !x.is_null())
        .map(|x| number(x, "blur_sigma_px_estimate"))
        .transpose()?;
    if blur.is_some_and(|x| x < 0.0) {
        return Err(invalid("blur sigma must be nonnegative"));
    }
    let n = detections.len();
    let mut report = json!({"schema":"fabric-photo-scale/1", "reference":"charuco", "coordinate_source":"imported_detections",
        "image_size_px":image, "image":v["image"], "paper":v["paper"], "board":board,
        "corners_detected":n,"corners_total":total,"markers_detected":markers,
        "blur_sigma_px_estimate":blur.map(|x| py::round(x, 2)), "ok":false,
        "notes":["Imported corner IDs and pixels: no image detection, EXIF processing or blur estimation was performed here.",
            "Print scale assumed 100%: confirm the 100 mm bar with a ruler (05 #9).", "Scale is exact only on the sheet's plane (03 §3.1)."], "warnings":[]});
    let mut warnings = Vec::new();
    if n < 6 {
        report["warnings"] = json!([format!(
            "BOARD_NOT_FOUND: {n} ChArUco corners (need ≥6; ≥12 for a usable scale)"
        )]);
        return Ok(PhotoScale {
            report,
            calibration: None,
        });
    }
    let p: Vec<Point> = detections
        .iter()
        .map(|(_, p)| undistort_point(*p, intrinsics.as_ref()))
        .collect::<Result<_, _>>()?;
    let q: Vec<Point> = detections
        .iter()
        .map(|(id, _)| {
            [
                (id % (sx - 1) + 1) as f64 * square,
                (id / (sx - 1) + 1) as f64 * square,
            ]
        })
        .collect();
    let (h, ids) = match robust(&p, &q) {
        Ok(value) => value,
        Err(e) => {
            report["warnings"] = json!([e.message]);
            return Ok(PhotoScale {
                report,
                calibration: None,
            });
        }
    };
    let mut s = plane_scale_in_image(&h, size, intrinsics.as_ref())?;
    if intrinsics.is_some() {
        // Perspective belongs to the pinhole plane; local pixel scale also includes the lens.
        s.tilt = plane_scale(&h, size)?.tilt;
    }
    let inv = h.inverse()?;
    let errors: Vec<_> = ids.iter().map(|&i| residual(&h, p[i], q[i])).collect();
    let sse: f64 = errors.iter().map(|v| v * v).sum();
    let rms = (sse / ids.len() as f64).sqrt();
    let pixel_errors = ids
        .iter()
        .map(|&i| {
            let projected = distort_point(inv.map(q[i])?, intrinsics.as_ref())?;
            let observed = detections[i].1;
            Ok(det::hypot(projected[0] - observed[0], projected[1] - observed[1]).powi(2))
        })
        .collect::<Result<Vec<_>, Refusal>>()?;
    let rms_px = (pixel_errors.iter().sum::<f64>() / ids.len() as f64).sqrt();
    Refusal::finite("reprojection RMS px", rms_px)?;
    report["ok"] = json!(true);
    scale_fields(&mut report, &h, &s);
    if let Some(k) = &intrinsics {
        // A nonlinear observed-pixel map cannot be represented by this 3x3 matrix. Do not
        // publish it under the old raw-pixel key where another consumer could misuse it.
        let matrix = report
            .as_object_mut()
            .unwrap()
            .remove("homography_px_to_board_mm")
            .unwrap();
        report["homography_undistorted_px_to_board_mm"] = matrix;
        report["intrinsics"] = json!(k);
        report["homography_coordinate_frame"] = json!("undistorted_pixels_same_intrinsics");
        report["scale_variation_sampling"] = json!("centre_and_four_board_corners");
        report["notes"].as_array_mut().unwrap().push(json!(
            "Brown lens correction applies to reference corners and landmark clicks. Scale and click sigma use observed-pixel Jacobians. Intrinsics are assumed exact; lens calibration uncertainty is not included."
        ));
    }
    report["inliers"] = json!(ids.len());
    report["inlier_ids"] = json!(ids.iter().map(|&i| detections[i].0).collect::<Vec<_>>());
    report["outlier_ids"] = json!(
        (0..n)
            .filter(|i| !ids.contains(i))
            .map(|i| detections[i].0)
            .collect::<Vec<_>>()
    );
    report["board_origin_on_paper_mm"] = json!(origin);
    report["reprojection_rms_mm"] = json!(py::round(rms, 4));
    report["reprojection_rms_px"] = json!(py::round(rms_px, 4));
    report["sigma_point_mm"] = json!(py::round((sse / (2 * ids.len() - 8) as f64).sqrt(), 4));
    report["max_residual_mm"] = json!(py::round(errors.iter().copied().fold(0.0, f64::max), 4));
    report["square_px"] = json!(py::round(square / s.mm_per_px, 2));
    report["ransac"] = json!({"threshold_mm":2.0,"max_trials":512,"seed":0,"refit":"normalized algebraic least squares"});
    // Only the consensus contributes scale evidence; rejected detections cannot suppress warnings.
    let retained = ids.len();
    if retained < 12 {
        warnings.push(format!(
            "TOO_FEW_CORNERS: {retained} of {total} inlier corners; need ≥12 for a reliable scale"
        ));
    } else if 2 * retained < total {
        warnings.push(format!("PARTIAL_BOARD: {retained} of {total} inlier corners — fingers, glare or the frame edge cover the sheet"));
    }
    if ids.len() < n {
        warnings.push(format!(
            "OUTLIER_CORNERS: excluded {} of {n} detected corners from calibration and uncertainty",
            n - ids.len()
        ));
    }
    let mut geometric = if intrinsics.is_some() {
        let mut perspective = plane_scale(&h, size)?;
        // Perspective warnings must not mistake radial distortion for a tilted reference.
        // Resolution/click diagnostics, however, belong to the observed image.
        perspective.mm_per_px = s.mm_per_px;
        scale_warnings(&perspective)
    } else {
        scale_warnings(&s)
    };
    let low = geometric
        .last()
        .is_some_and(|w| w.starts_with("LOW_RESOLUTION"))
        .then(|| geometric.pop().unwrap());
    warnings.extend(geometric);
    if let Some(blur) = blur.filter(|b| *b > 2.0) {
        warnings.push(format!(
            "BLURRY: edge blur ≈{} px; use burst/timer, more light",
            py::fixed(blur, 1)
        ));
    }
    if rms > 0.5 {
        warnings.push(format!(
            "HIGH_RESIDUAL: {} mm RMS — sheet not flat (tape it to card) or strong lens distortion",
            py::fixed(rms, 2)
        ));
    }
    warnings.extend(low);
    report["warnings"] = json!(warnings);
    Ok(PhotoScale {
        report,
        calibration: Some(Calibration {
            h,
            size,
            image,
            samples: Samples::Charuco {
                pixels: ids.iter().map(|&i| p[i]).collect(),
                board: ids.iter().map(|&i| q[i]).collect(),
                observed: ids.iter().map(|&i| detections[i].1).collect(),
            },
            intrinsics,
            intrinsic_uncertainty: None,
        }),
    })
}
