//! Photo scale from reference coordinates or detected plain-paper edges (spec 02 A.10–12).
//!
//! Coordinates and pixels must already use the full-resolution, EXIF-oriented image frame.
//! Image decoding stays with the caller. A homography measures only the sheet's plane.

pub mod acquisition;
pub mod calibration_images;
mod camera;
mod charuco;
mod charuco_detect;
mod detect;
mod geometry;
pub mod intrinsics;
mod lens_uncertainty;
mod measurement;

pub use camera::Camera;
pub use charuco_detect::detect_charuco;
pub use detect::{MAX_PHOTO_PIXELS, analyze_plain_sheet};
pub use measurement::{PhotoOptions, measure_photo};

use ftx_core::{Refusal, RefusalCode, py};
use serde_json::{Value, json};

use super::invalid;
use geometry::{
    Homography, PlaneScale, Point, convex_clockwise, fit, plane_scale, rectangle,
    rectangle_residual,
};

const RECT_OK: f64 = 0.025;

/// Scale report and the unrounded calibration needed to measure landmarks. Rejected or ambiguous
/// references have a report with `ok: false` and cannot be used for measurements.
#[derive(Clone, Debug)]
pub struct PhotoScale {
    report: Value,
    calibration: Option<Calibration>,
}

#[derive(Clone, Debug)]
struct Calibration {
    h: Homography,
    size: Point,
    image: Point,
    samples: Samples,
    // When present, h and samples use virtual pinhole pixels; clicks/receipts stay observed.
    intrinsics: Option<crate::camera::Intrinsics>,
    intrinsic_uncertainty: Option<intrinsics::IntrinsicUncertainty>,
}

#[derive(Clone, Debug)]
enum Samples {
    Manual {
        corners: [Point; 4],
        sigma: f64,
    },
    Charuco {
        pixels: Vec<Point>,
        board: Vec<Point>,
        observed: Vec<Point>,
    },
    Edges {
        points: [Vec<Point>; 4],
    },
}

impl PhotoScale {
    /// Public `fabric-photo-scale/1` report; its rounded matrix is for interchange, not calculations.
    #[must_use]
    pub fn report(&self) -> &Value {
        &self.report
    }
}

fn number(v: &Value, label: &str) -> Result<f64, Refusal> {
    v.as_f64()
        .filter(|n| n.is_finite())
        .ok_or_else(|| invalid(format!("{label} must be a finite number")))
}

fn positive(v: &Value, label: &str) -> Result<f64, Refusal> {
    let n = number(v, label)?;
    if n > 0.0 {
        Ok(n)
    } else {
        Err(invalid(format!("{label} must be positive")))
    }
}

fn point(v: &Value, label: &str) -> Result<Point, Refusal> {
    let a = v
        .as_array()
        .filter(|a| a.len() == 2)
        .ok_or_else(|| invalid(format!("{label} must be [x, y]")))?;
    Ok([number(&a[0], label)?, number(&a[1], label)?])
}

fn known(v: &Value, names: &[&str]) -> Result<(), Refusal> {
    let obj = v
        .as_object()
        .ok_or_else(|| invalid("photo reference must be an object"))?;
    for key in obj.keys() {
        if !names.contains(&key.as_str()) {
            return Err(Refusal::new(
                RefusalCode::UnknownParameter,
                format!("unknown photo reference field '{key}'"),
            ));
        }
    }
    Ok(())
}

fn round_point(p: Point, digits: usize) -> Point {
    p.map(|x| py::round(x, digits))
}

fn scale_fields(report: &mut Value, h: &Homography, s: &PlaneScale) {
    report["homography_px_to_board_mm"] = json!(h.0.map(|r| r.map(|v| py::round(v, 10))));
    report["board_centre_px"] = json!(round_point(s.centre, 2));
    report["mm_per_px"] = json!(py::round(s.mm_per_px, 6));
    report["px_per_mm"] = json!(py::round(1.0 / s.mm_per_px, 4));
    report["mm_per_px_axes"] = json!(round_point(s.axes, 6));
    report["tilt_deg_estimate"] = json!(py::round(s.tilt, 2));
    report["scale_variation_across_board"] = json!(py::round(s.variation, 4));
}

fn scale_warnings(s: &PlaneScale) -> Vec<String> {
    let mut out = Vec::new();
    if s.tilt > 10.0 {
        out.push(format!("STRONG_PERSPECTIVE: sheet ≈{}° off the image plane (10° ≈ 1.5% foreshortening, 03 §3.1); body widths via this sheet will be off — turn the sheet square to the camera", py::fixed(s.tilt, 0)));
    }
    if s.variation > 0.05 {
        out.push(format!("STRONG_PERSPECTIVE: scale varies {}% across the sheet (too close / wide lens?); use the 2× lens from ~2.5 m", py::fixed(100.0 * s.variation, 1)));
    }
    if s.mm_per_px > 1.0 {
        out.push(format!(
            "LOW_RESOLUTION: {} mm/px at the sheet; each landmark click is ±{} mm",
            py::fixed(s.mm_per_px, 2),
            py::fixed(3.0 * s.mm_per_px, 0)
        ));
    }
    out
}

struct PaperFit {
    size: Point,
    landscape: bool,
    residual: f64,
    focal: f64,
    h: Homography,
    scale: PlaneScale,
}

fn paper_fit(corners: &[Point; 4], paper: &str, image: Point) -> Result<PaperFit, Refusal> {
    let size = if paper == "letter" {
        [215.9, 279.4]
    } else {
        [210.0, 297.0]
    };
    let portrait = rectangle_residual(corners, size, image)?;
    let landscape = rectangle_residual(corners, [size[1], size[0]], image)?;
    let turned = landscape.0 < portrait.0;
    let (size, (residual, focal)) = if turned {
        ([size[1], size[0]], landscape)
    } else {
        (size, portrait)
    };
    let h = fit(corners, &rectangle(size))?;
    let scale = plane_scale(&h, size)?;
    Ok(PaperFit {
        size,
        landscape: turned,
        residual,
        focal,
        h,
        scale,
    })
}

/// Analyze a `ftx-photo-reference/1` document with `reference: "plain_sheet"`, `paper`
/// (`letter`, `a4`, or `auto`), `image_size_px: [width,height]`, and four `corners_px`
/// clockwise on screen. `corner_sigma_px` defaults to 3, describing manual corner uncertainty.
///
/// A rectangle/aspect check is mandatory: four arbitrary points alone are not evidence of scale.
/// A wrong or ambiguous paper produces `ok: false`, not a usable homography. No automatic image
/// detection, blur estimation or EXIF processing is performed at this coordinate-only boundary.
/// Imported detector output can instead use `reference: "charuco"`, `board` dimensions, and
/// `corners: [{id, point_px}]`; at least six identified corners must support a single homography.
/// Optional `intrinsics` on a ChArUco reference uses the shared camera model to remove lens
/// distortion from both corners and subsequent clicks. They must describe exactly this oriented
/// image. The matrix is then named `homography_undistorted_px_to_board_mm`, not a raw-pixel map.
/// Intrinsics are treated as fixed, not newly estimated or assigned zero statistical uncertainty.
pub fn analyze_reference(reference: &Value) -> Result<PhotoScale, Refusal> {
    if reference["reference"] == "charuco" {
        return charuco::analyze(reference);
    }
    known(
        reference,
        &[
            "schema",
            "reference",
            "paper",
            "image_size_px",
            "corners_px",
            "corner_sigma_px",
            "image",
        ],
    )?;
    if reference["schema"] != "ftx-photo-reference/1" || reference["reference"] != "plain_sheet" {
        return Err(invalid(
            "expected ftx-photo-reference/1 with reference plain_sheet",
        ));
    }
    if reference.get("image").is_some_and(|v| !v.is_string()) {
        return Err(invalid("image must be a source image label"));
    }
    let image = point(&reference["image_size_px"], "image_size_px")?;
    if image
        .iter()
        .any(|v| *v < 2.0 || *v > 1_000_000.0 || v.fract() != 0.0)
    {
        return Err(invalid(
            "image_size_px must contain integer dimensions in 2..=1000000",
        ));
    }
    let paper = reference["paper"].as_str().unwrap_or("");
    if !["letter", "a4", "auto"].contains(&paper) {
        return Err(invalid("paper must be letter, a4 or auto"));
    }
    let values = reference["corners_px"]
        .as_array()
        .filter(|a| a.len() == 4)
        .ok_or_else(|| invalid("corners_px needs four [x, y] points clockwise on screen"))?;
    let mut corners = [[0.0; 2]; 4];
    for (p, value) in corners.iter_mut().zip(values) {
        *p = point(value, "corners_px")?;
        if (0..2).any(|i| p[i] < 0.0 || p[i] > image[i] - 1.0) {
            return Err(invalid(
                "reference corners must lie inside the oriented full-resolution image",
            ));
        }
    }
    if !convex_clockwise(&corners) {
        return Err(geometry::degenerate(
            "sheet corners must form a convex clockwise quadrilateral",
        ));
    }
    let sigma = match reference.get("corner_sigma_px") {
        Some(v) => positive(v, "corner_sigma_px")?,
        None => 3.0,
    };
    let letter = paper_fit(&corners, "letter", image)?;
    let a4 = paper_fit(&corners, "a4", image)?;
    let label = |p| if p == "letter" { "Letter" } else { "A4" };
    let chosen = if paper == "auto" {
        if letter.residual <= a4.residual {
            "letter"
        } else {
            "a4"
        }
    } else {
        paper
    };
    let (best, other, other_name) = if chosen == "letter" {
        (&letter, &a4, "a4")
    } else {
        (&a4, &letter, "letter")
    };
    let mut report = json!({"schema":"fabric-photo-scale/1", "reference":"plain_sheet",
        "coordinate_source":"manual_corners", "paper_requested":paper, "paper":chosen,
        "image_size_px":image, "corners_px":corners.map(|p| round_point(p, 2)),
        "corner_sigma_px":sigma, "sheet_size_sigma_mm":0.5, "ok":false,
        "orientation":if best.landscape {"landscape"} else {"portrait"},
        "rect_residual":py::round(best.residual, 5),
        "rect_residual_by_paper":{"letter":py::round(letter.residual, 5),"a4":py::round(a4.residual, 5)},
        "edge_fit_rms_px":null, "blur_sigma_px_estimate":null,
        "notes":["Manual sheet corners: no image detection or straight-edge/blur check was performed. Confirm the marked corners and nominal paper size.",
            "Sheet size assumed nominal. Scale is exact only on the sheet's plane (03 §3.1).", "Coordinates must use the full-resolution, EXIF-oriented image frame."], "warnings":[]});
    if let Some(image) = reference.get("image") {
        report["image"] = image.clone();
    }
    let mut warnings = Vec::new();
    if best.residual > RECT_OK {
        if paper != "auto" && other.residual <= RECT_OK.min(best.residual / 2.0) {
            warnings.push(format!("ASPECT_MISMATCH: asked for {} but the sheet fits {} (rect_residual {} vs {}) — pass --paper {other_name}", label(chosen), label(other_name), py::fixed(best.residual, 3), py::fixed(other.residual, 3)));
        } else {
            warnings.push(format!("NOT_A_SHEET: the best rectangle misses {} by {} (limit 0.025) — check the paper size, flatness and marked corners", label(chosen), py::fixed(best.residual, 3)));
        }
    } else if paper == "auto" && other.residual <= RECT_OK {
        report["paper"] = Value::Null;
        warnings.push(format!("PAPER_AMBIGUOUS: fits {} tilted ≈{}° and {} tilted ≈{}° — four corners cannot tell a tilted sheet of one from the other; specify letter or a4", label(chosen), py::fixed(best.scale.tilt, 0), label(other_name), py::fixed(other.scale.tilt, 0)));
    } else {
        report["ok"] = json!(true);
        scale_fields(&mut report, &best.h, &best.scale);
        report["focal_px_estimate"] = if best.scale.variation >= 0.02 {
            json!(py::round(best.focal, 1))
        } else {
            Value::Null
        };
        if other.residual <= RECT_OK && best.scale.tilt - other.scale.tilt > 10.0 {
            warnings.push(format!(
                "PAPER_AMBIGUOUS: the marked sheet also fits {}; check which paper it is",
                label(other_name)
            ));
        }
        warnings.extend(scale_warnings(&best.scale));
    }
    report["warnings"] = json!(warnings);
    let calibration = (report["ok"] == true).then(|| Calibration {
        h: best.h.clone(),
        size: best.size,
        image,
        samples: Samples::Manual { corners, sigma },
        intrinsics: None,
        intrinsic_uncertainty: None,
    });
    Ok(PhotoScale {
        report,
        calibration,
    })
}
