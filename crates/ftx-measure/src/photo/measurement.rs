//! Conditional uncertainty in the sheet plane. Manual-corner bootstrap includes corner clicks
//! and nominal paper-size error. Detected sheets bootstrap their supported straight-edge samples.

use ftx_core::{Refusal, RefusalCode, det, py};
use serde_json::{Value, json};

use super::geometry::{
    Homography, Point, convex_clockwise, fit, image_jacobian, plane_scale, plane_scale_in_image,
    rectangle, undistort_point,
};
use super::{Calibration, PhotoScale, Samples, invalid, known, number, point, round_point};

/// Measurement uncertainty and coordinate-frame controls. Lengths are mm, angles degrees,
/// pixel values refer to the full-resolution oriented image. No system clock or ambient RNG.
#[derive(Clone, Debug)]
pub struct PhotoOptions {
    /// Uncertain distance between the landmark plane and reference plane; not a scale correction.
    pub depth_offset_mm: f64,
    /// Distance from camera to sheet for the depth uncertainty term.
    pub camera_distance_mm: f64,
    /// One-sigma uncertainty of each landmark click, in full-resolution pixels.
    pub click_px: f64,
    /// View label included in each method, e.g. P1 or P3.
    pub view: Option<String>,
    /// Width of the resized image used for clicking, or None for full-resolution coordinates.
    pub points_from_width: Option<f64>,
    /// One-sigma uncertainty that the sheet is level, used for angles only.
    pub sheet_level_sigma_deg: f64,
    /// Number of bootstrap draws; must be 20..=2000. At least 80% must be usable.
    pub bootstrap: usize,
    /// Deterministic seed (SplitMix64, Box–Muller; not NumPy's random stream).
    pub seed: u64,
}

impl Default for PhotoOptions {
    fn default() -> Self {
        Self {
            depth_offset_mm: 0.0,
            camera_distance_mm: 2500.0,
            click_px: 3.0,
            view: None,
            points_from_width: None,
            sheet_level_sigma_deg: 1.5,
            bootstrap: 200,
            seed: 0,
        }
    }
}

pub(super) struct Random {
    pub(super) state: u64,
}
impl Random {
    pub(super) fn uniform(&mut self) -> f64 {
        self.state = self.state.wrapping_add(0x9e3779b97f4a7c15);
        let mut x = self.state;
        x = (x ^ (x >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        x = (x ^ (x >> 27)).wrapping_mul(0x94d049bb133111eb);
        x ^= x >> 31;
        ((x >> 12) as f64 + 0.5) / 4503599627370496.0
    }
    fn normal(&mut self) -> f64 {
        let radius = (-2.0 * det::ln(self.uniform())).sqrt();
        radius * det::cos(2.0 * std::f64::consts::PI * self.uniform())
    }
}

fn bootstrap(c: &Calibration, options: &PhotoOptions) -> Result<Vec<Homography>, Refusal> {
    let mut rng = Random {
        state: options.seed,
    };
    let mut out = Vec::with_capacity(options.bootstrap);
    for _ in 0..options.bootstrap {
        let (trial, size) = match &c.samples {
            Samples::Manual { corners, sigma } => {
                let corners = corners.map(|p| p.map(|v| v + sigma * rng.normal()));
                let size = c.size.map(|v| v + 0.5 * rng.normal());
                if !convex_clockwise(&corners) || size.iter().any(|&x| x <= 0.0) {
                    continue;
                }
                (fit(&corners, &rectangle(size)), size)
            }
            Samples::Charuco { pixels, board } => {
                let n = pixels.len();
                let ids: Vec<usize> = (0..n)
                    .map(|_| (rng.uniform() * n as f64) as usize)
                    .collect();
                let mut unique = ids.clone();
                unique.sort_unstable();
                unique.dedup();
                if unique.len() < 6 {
                    continue;
                }
                let p: Vec<_> = ids.iter().map(|&i| pixels[i]).collect();
                let q: Vec<_> = ids.iter().map(|&i| board[i]).collect();
                (fit(&p, &q), c.size)
            }
            Samples::Edges { points } => {
                let lines: Option<Vec<_>> = points
                    .iter()
                    .map(|edge| {
                        let sample: Vec<_> = (0..edge.len())
                            .map(|_| edge[(rng.uniform() * edge.len() as f64) as usize])
                            .collect();
                        super::detect::line_fit(&sample)
                    })
                    .collect();
                let Some(lines) = lines else {
                    continue;
                };
                let lines = lines.try_into().expect("four sheet sides");
                let Some(corners) = super::detect::corners_of(&lines) else {
                    continue;
                };
                let size = c.size.map(|v| v + 0.5 * rng.normal());
                if size.iter().any(|&v| v <= 0.0) {
                    continue;
                }
                (fit(&corners, &rectangle(size)), size)
            }
        };
        if let Ok(h) = trial
            && plane_scale_in_image(&h, size, c.intrinsics.as_ref()).is_ok()
        {
            out.push(h);
        }
    }
    if out.len() * 5 < options.bootstrap * 4 {
        return Err(Refusal::new(
            RefusalCode::NotConverged,
            "too few usable photo bootstrap draws; reference geometry is unstable",
        )
        .repair("mark a larger, less tilted sheet; for ChArUco expose at least twelve well-spread corners"));
    }
    Ok(out)
}

fn on_plane(h: &Homography, p: Point, centre: Point) -> Result<Point, Refusal> {
    if h.denominator(p)?.is_sign_positive() != h.denominator(centre)?.is_sign_positive() {
        return Err(super::geometry::degenerate(
            "landmark lies across the sheet's projective horizon",
        ));
    }
    h.map(p)
}

fn deviation(values: &[f64]) -> Result<f64, Refusal> {
    let n = values.len() as f64;
    let mean: f64 = values.iter().map(|v| v / n).sum();
    let sigma = values
        .iter()
        .map(|v| ((v - mean) / n.sqrt()).powi(2))
        .sum::<f64>()
        .sqrt();
    Refusal::finite("homography uncertainty", sigma)
}

fn checked_options(o: &PhotoOptions) -> Result<(), Refusal> {
    for (name, v) in [
        ("depth_offset_mm", o.depth_offset_mm),
        ("camera_distance_mm", o.camera_distance_mm),
        ("click_px", o.click_px),
        ("sheet_level_sigma_deg", o.sheet_level_sigma_deg),
    ] {
        Refusal::finite(name, v)?;
    }
    if o.camera_distance_mm <= 0.0 || o.click_px <= 0.0 || o.sheet_level_sigma_deg < 0.0 {
        return Err(invalid(
            "camera distance and click sigma must be positive; sheet-level sigma must be nonnegative",
        ));
    }
    if o.depth_offset_mm.abs() >= o.camera_distance_mm {
        return Err(invalid(
            "depth offset magnitude must be less than camera distance",
        ));
    }
    if o.points_from_width
        .is_some_and(|v| !v.is_finite() || v <= 0.0)
    {
        return Err(invalid("points_from_width must be positive and finite"));
    }
    if !(20..=2000).contains(&o.bootstrap) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "bootstrap must be in 20..=2000",
        ));
    }
    Ok(())
}

/// Measure named pairs of landmarks using an accepted scale, producing a new
/// `fabric-measurements/1` document. Distances/angles and decomposed one-sigma uncertainty
/// follow spec 02 A.12; every result remains `PHOTO_ESTIMATED` through fusion/readiness.
///
/// The caller supplies a points object: `name[@tag]` -> two pixel points, or an object with
/// `points`, optional `view`, `depth_offset_mm`, and `definition_id`. Invalid requests are
/// atomic refusals. An ambiguous scale or unstable bootstrap never publishes a partial file.
pub fn measure_photo(
    scale: &PhotoScale,
    points: &Value,
    options: &PhotoOptions,
) -> Result<Value, Refusal> {
    checked_options(options)?;
    let c = scale.calibration.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::Unscaled,
            scale.report["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; "),
        )
    })?;
    let entries = points
        .as_object()
        .filter(|m| !m.is_empty())
        .ok_or_else(|| invalid("points must be a nonempty object of named pairs"))?;
    if entries.len() > 512 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "at most 512 landmark pairs per photo",
        ));
    }
    let hs = bootstrap(c, options)?;
    let centre = plane_scale(&c.h, c.size)?.centre;
    let factor = c.image[0] / options.points_from_width.unwrap_or(c.image[0]);
    let reference_name = if matches!(c.samples, Samples::Charuco { .. }) {
        "ChArUco"
    } else if scale.report["paper"] == "letter" {
        "plain Letter"
    } else {
        "plain A4"
    };
    let mut records = Vec::with_capacity(entries.len());
    for (key, spec) in entries {
        let (name, tag) = key
            .split_once('@')
            .map_or((key.as_str(), None), |(n, t)| (n, Some(t)));
        if name.trim().is_empty() {
            return Err(invalid("photo measurement needs a name before @"));
        }
        let pairs = if spec.is_object() {
            known(
                spec,
                &["points", "view", "depth_offset_mm", "definition_id"],
            )?;
            &spec["points"]
        } else {
            spec
        };
        let pairs = pairs
            .as_array()
            .filter(|a| a.len() == 2)
            .ok_or_else(|| invalid(format!("{key}: need exactly two [x, y] points")))?;
        let p = point(&pairs[0], key)?.map(|v| v * factor);
        let q = point(&pairs[1], key)?.map(|v| v * factor);
        if [p, q]
            .iter()
            .any(|p| (0..2).any(|i| !p[i].is_finite() || p[i] < 0.0 || p[i] > c.image[i] - 1.0))
        {
            return Err(invalid(format!(
                "{key}: landmark outside the full-resolution image"
            )));
        }
        let text_field = |k: &str| -> Result<Option<&str>, Refusal> {
            if !spec.is_object() {
                return Ok(None);
            }
            match spec.get(k) {
                None => Ok(None),
                Some(v) => v
                    .as_str()
                    .map(Some)
                    .ok_or_else(|| invalid(format!("{key}: {k} must be a string"))),
            }
        };
        let view = text_field("view")?
            .or(options.view.as_deref())
            .unwrap_or("?");
        let did = text_field("definition_id")?;
        let depth = if spec.is_object() && spec.get("depth_offset_mm").is_some() {
            number(&spec["depth_offset_mm"], "depth_offset_mm")?
        } else {
            options.depth_offset_mm
        };
        if depth.abs() >= options.camera_distance_mm {
            return Err(invalid(format!(
                "{key}: depth offset magnitude must be less than camera distance"
            )));
        }
        let up = undistort_point(p, c.intrinsics.as_ref())?;
        let uq = undistort_point(q, c.intrinsics.as_ref())?;
        let (a, b) = (on_plane(&c.h, up, centre)?, on_plane(&c.h, uq, centre)?);
        let delta = [b[0] - a[0], b[1] - a[1]];
        let dist = Refusal::finite("sheet-plane distance", det::hypot(delta[0], delta[1]))?;
        if dist <= 1e-12 {
            return Err(super::geometry::degenerate(&format!(
                "{key}: the two points coincide"
            )));
        }
        let angle = crate::unit(&crate::canonical_name(name)) == "deg";
        let value_of = |a: Point, b: Point| {
            if angle {
                det::atan2((b[1] - a[1]).abs(), (b[0] - a[0]).abs()).to_degrees()
            } else {
                det::hypot(b[0] - a[0], b[1] - a[1])
            }
        };
        let mut draws = Vec::with_capacity(hs.len());
        for h in &hs {
            if let (Ok(a), Ok(b)) = (on_plane(h, up, centre), on_plane(h, uq, centre)) {
                let v = value_of(a, b);
                if v.is_finite() {
                    draws.push(v);
                }
            }
        }
        if draws.len() * 5 < options.bootstrap * 4 {
            return Err(Refusal::new(
                RefusalCode::NotConverged,
                format!("{key}: bootstrap unstable at the landmark location"),
            ));
        }
        let sa = deviation(&draws)?;
        let u = if angle {
            [-delta[1] / dist, delta[0] / dist]
        } else {
            [delta[0] / dist, delta[1] / dist]
        };
        let projected = |j: [[f64; 2]; 2]| {
            (u[0] * j[0][0] + u[1] * j[1][0]).powi(2) + (u[0] * j[0][1] + u[1] * j[1][1]).powi(2)
        };
        let mut sc = options.click_px
            * (projected(image_jacobian(&c.h, p, c.intrinsics.as_ref())?)
                + projected(image_jacobian(&c.h, q, c.intrinsics.as_ref())?))
            .sqrt();
        if angle {
            sc = (sc / dist).to_degrees();
        }
        let sb = if angle {
            options.sheet_level_sigma_deg
        } else {
            dist * depth.abs() / options.camera_distance_mm
        };
        let sigma = Refusal::finite(
            "photo measurement sigma",
            det::hypot(det::hypot(sa, sb), sc),
        )?;
        let mut method = format!("photo {view} {reference_name} sheet-plane homography");
        if let Some(tag) = tag {
            method.push_str(&format!(" [{tag}]"));
        }
        method.push_str(if angle {
            "; angle below horizontal in the sheet plane (sheet assumed upright)"
        } else {
            "; sheet-plane distance"
        });
        let mut notes = if angle {
            format!(
                "σ terms deg: homography {}, click {} ({} px), sheet level {}",
                py::fixed(sa, 2),
                py::fixed(sc, 2),
                options.click_px,
                options.sheet_level_sigma_deg
            )
        } else {
            format!(
                "σ terms mm: homography {}, depth {} (Δz {} at Z₀ {}), click {} ({} px)",
                py::fixed(sa, 2),
                py::fixed(sb, 2),
                depth,
                options.camera_distance_mm,
                py::fixed(sc, 2),
                options.click_px
            )
        };
        if crate::definition(&crate::canonical_name(name)).is_none() {
            notes.push_str("; name not in the catalogue");
        }
        let digits = if angle { 2 } else { 1 };
        let mut record = crate::make_record(
            name,
            py::round(value_of(a, b), digits),
            py::round(sigma, digits),
            "PHOTO_ESTIMATED",
            &method,
            did,
            &notes,
        )?;
        record["points_px"] = json!([round_point(p, 2), round_point(q, 2)]);
        let terms = if angle {
            json!({"homography":py::round(sa, 3), "click":py::round(sc, 3), "sheet_level":sb})
        } else {
            json!({"homography":py::round(sa, 3), "depth_offset":py::round(sb, 3), "click":py::round(sc, 3)})
        };
        record[if angle {
            "sigma_terms_deg"
        } else {
            "sigma_terms_mm"
        }] = terms;
        record["bootstrap_accepted"] = json!(draws.len());
        records.push(record);
    }
    let mut report = scale.report.clone();
    let bootstrap_method = match c.samples {
        Samples::Manual { .. } => {
            "manual-corner Gaussian perturbation and 0.5 mm paper-side sigma; SplitMix64/Box-Muller"
        }
        Samples::Charuco { .. } => {
            "resample identified inlier corners with replacement; at least six unique points per draw; SplitMix64"
        }
        Samples::Edges { .. } => {
            "resample detected inlier edge points, TLS lines, intersect corners and 0.5 mm paper-side sigma; SplitMix64/Box-Muller"
        }
    };
    report["bootstrap"] = json!({"requested":options.bootstrap, "calibrations":hs.len(), "seed":options.seed,
        "method":bootstrap_method});
    report["measure_warnings"] = if options.depth_offset_mm == 0.0 {
        json!([
            "depth offset 0 assumed: valid only for landmarks in the sheet's plane (sheet beside the child, centre over the same floor line; 03 §3.1). Otherwise pass --depth-offset-mm."
        ])
    } else {
        json!([])
    };
    Ok(
        json!({"schema":"fabric-measurements/1", "wearer":{}, "taken_on":null, "records":records, "fused":{}, "photo":report}),
    )
}
