//! Width/depth section model (spec 02 A.5). A k-corrected ellipse is a prediction, not a tape
//! measurement; uncertain shape factors remain explicit even for exact image coordinates.

use ftx_core::{Refusal, det, py};
use serde_json::{Value, json};

use crate::invalid;

/// Section geometry, shape-factor uncertainty and optional per-wearer calibration.
#[derive(Clone, Debug)]
pub struct SectionOptions<'a> {
    /// Anatomical level; chest, waist and hip have explicitly uncertain defaults.
    pub level: &'a str,
    /// Width uncertainty in mm (one sigma).
    pub sigma_width_mm: f64,
    /// Depth uncertainty in mm (one sigma).
    pub sigma_depth_mm: f64,
    /// Optional explicit multiplier of the ellipse perimeter.
    pub k: Option<f64>,
    /// Optional one-sigma shape-factor uncertainty.
    pub k_sigma: Option<f64>,
    /// Per-wearer multiplier of k, e.g. returned by `calibrate_k`.
    pub k_ratio: f64,
    /// One-sigma uncertainty of that multiplier.
    pub k_ratio_sigma: f64,
    /// Clothing thickness subtracted from both sides of width and depth, in mm.
    pub clothing_mm: f64,
}

impl Default for SectionOptions<'_> {
    fn default() -> Self {
        Self {
            level: "waist",
            sigma_width_mm: 0.0,
            sigma_depth_mm: 0.0,
            k: None,
            k_sigma: None,
            k_ratio: 1.0,
            k_ratio_sigma: 0.0,
            clothing_mm: 0.0,
        }
    }
}

fn factor(level: &str) -> Option<(f64, f64, &'static str)> {
    match level {
        "waist" => Some((
            1.06,
            0.022,
            "ANSUR II adult fit k=1.054 M / 1.061 F (03 §3.3); UNCERTAIN for children",
        )),
        "hip" => Some((
            1.09,
            0.020,
            "ANSUR II adult fit k=1.089 M / 1.097 F (03 §3.3); UNCERTAIN for children",
        )),
        "chest" => Some((
            1.07,
            0.030,
            "UNCERTAIN: toddler chest k≈1.05–1.10 assumed; adult chest k 1.16–1.24 does not transfer (03 §3.3)",
        )),
        _ => None,
    }
}

fn positive(name: &str, value: f64) -> Result<f64, Refusal> {
    Refusal::finite(name, value)?;
    if value > 0.0 {
        Ok(value)
    } else {
        Err(invalid(format!("{name} must be positive")))
    }
}

fn nonnegative(name: &str, value: f64) -> Result<f64, Refusal> {
    Refusal::finite(name, value)?;
    if value >= 0.0 {
        Ok(value)
    } else {
        Err(invalid(format!("{name} must be nonnegative")))
    }
}

fn perimeter(w: f64, d: f64) -> Result<f64, Refusal> {
    positive("unclothed width", w)?;
    positive("unclothed depth", d)?;
    let (a, b) = (w / 2.0, d / 2.0);
    let h = ((a - b) / (a + b)).powi(2);
    positive(
        "ellipse perimeter",
        std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt())),
    )
}

/// Infer a girth from a full width/depth in mm, with separate width, depth and shape-factor
/// uncertainty. Follows the spec's central differences and published rounding. Non-finite,
/// negative uncertainty, impossible clothing offsets and unknown shape defaults are refused.
pub fn girth_from_section(
    width: f64,
    depth: f64,
    o: &SectionOptions<'_>,
) -> Result<Value, Refusal> {
    positive("width", width)?;
    positive("depth", depth)?;
    positive("k_ratio", o.k_ratio)?;
    nonnegative("sigma_width_mm", o.sigma_width_mm)?;
    nonnegative("sigma_depth_mm", o.sigma_depth_mm)?;
    nonnegative("k_ratio_sigma", o.k_ratio_sigma)?;
    nonnegative("clothing_mm", o.clothing_mm)?;
    let (base, spread, note) = match factor(o.level) {
        Some(v) => v,
        None => (
            o.k.ok_or_else(|| {
                invalid(format!(
                    "level '{}' has no default k; pass k (and k_sigma)",
                    o.level
                ))
            })?,
            0.03,
            "user k",
        ),
    };
    let base = positive("k", o.k.unwrap_or(base))?;
    let spread = nonnegative("k_sigma", o.k_sigma.unwrap_or(spread))?;
    let k = positive("effective k", base * o.k_ratio)?;
    let sk = Refusal::finite(
        "effective k sigma",
        det::hypot(spread * o.k_ratio, base * o.k_ratio_sigma),
    )?;
    let (w, d) = (width - 2.0 * o.clothing_mm, depth - 2.0 * o.clothing_mm);
    let p = perimeter(w, d)?;
    let h = 1e-3 * w.max(d);
    let dw = (perimeter(w + h, d)? - perimeter(w - h, d)?) / (2.0 * h);
    let dd = (perimeter(w, d + h)? - perimeter(w, d - h)?) / (2.0 * h);
    let terms = [k * dw * o.sigma_width_mm, k * dd * o.sigma_depth_mm, p * sk];
    let sigma = Refusal::finite(
        "girth sigma",
        det::hypot(det::hypot(terms[0], terms[1]), terms[2]),
    )?;
    let girth = positive("girth", k * p)?;
    let mut notes = note.to_string();
    if o.clothing_mm > 0.0 {
        notes
            .push_str("; clothing offset subtracted (2–5 mm per side for thin clothes, UNCERTAIN)");
    }
    Ok(
        json!({"level":o.level, "girth_mm":py::round(girth, 1), "sigma_mm":py::round(sigma, 1),
        "ellipse_perimeter_mm":py::round(p, 1), "k":py::round(k, 4), "k_sigma":py::round(sk, 4),
        "sigma_terms_mm":{"width":py::round(terms[0].abs(), 2), "depth":py::round(terms[1].abs(), 2), "k":py::round(terms[2].abs(), 2)},
        "width_mm":width, "depth_mm":depth, "clothing_mm_per_side":o.clothing_mm,"notes":notes}),
    )
}

/// Calibrate the section shape factor against an independent tape girth. This estimates a model
/// coefficient, not another independent observation of the same girth. The reference uses the
/// rounded perimeter/sigma from the section report here; retain that documented behavior.
pub fn calibrate_k(
    tape: f64,
    width: f64,
    depth: f64,
    sigma_tape: f64,
    o: &SectionOptions<'_>,
) -> Result<Value, Refusal> {
    positive("tape_girth_mm", tape)?;
    positive("sigma_tape_mm", sigma_tape)?;
    let unit = SectionOptions {
        k: Some(1.0),
        k_sigma: Some(0.0),
        k_ratio: 1.0,
        k_ratio_sigma: 0.0,
        ..o.clone()
    };
    let ellipse = girth_from_section(width, depth, &unit)?;
    let p = positive(
        "rounded ellipse perimeter",
        ellipse["ellipse_perimeter_mm"].as_f64().unwrap(),
    )?;
    let sp = ellipse["sigma_mm"].as_f64().unwrap();
    let k = positive("calibrated k", tape / p)?;
    let sigma = Refusal::finite(
        "calibrated k sigma",
        k * det::hypot(sigma_tape / tape, sp / p),
    )?;
    let default = factor(o.level).map_or(1.0, |(k, _, _)| k);
    Ok(
        json!({"level":o.level,"k_child":py::round(k, 4), "k_child_sigma":py::round(sigma, 4),
        "k_default":default,"k_ratio":py::round(k / default, 4), "k_ratio_sigma":py::round(sigma / default, 4),
        "use":"pass --k-ratio/--k-ratio-sigma to girth-from-section for this child's other levels"}),
    )
}
