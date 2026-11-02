//! Width/depth section model (spec 02 A.5). A k-corrected ellipse is a prediction, not a tape
//! measurement; uncertain shape factors remain explicit even for exact image coordinates.

use ftx_core::{Refusal, RefusalCode, det, py, receipt::sha256_hex};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{canonical_name, definition, fuse, invalid, make_record, validate};

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

/// The section estimate as an ordinary PHOTO_ESTIMATED measurement document when its name is
/// known. Numeric callers and source-qualified photo callers share this record construction.
///
/// # Errors
/// Invalid section geometry, uncertainty or a name belonging to a non-girth measurement.
pub fn section_measurement(
    width: f64,
    depth: f64,
    options: &SectionOptions<'_>,
    name: Option<&str>,
) -> Result<Value, Refusal> {
    let mut out = girth_from_section(width, depth, options)?;
    let Some(name) = name.or_else(|| {
        ["chest", "waist", "hip"]
            .contains(&options.level)
            .then_some(options.level)
    }) else {
        return Ok(out);
    };
    let name = canonical_name(name);
    if definition(&name).is_some_and(|d| d["kind"] != "girth") || name.ends_with("_deg") {
        return Err(invalid(
            "the derived record name must identify a girth, not a width, length or angle",
        ));
    }
    let method = format!(
        "girth-from-section k·Ramanujan(width {width}, depth {depth}), k={}",
        out["k"]
    );
    let record = make_record(
        &name,
        out["girth_mm"].as_f64().unwrap(),
        out["sigma_mm"].as_f64().unwrap(),
        "PHOTO_ESTIMATED",
        &method,
        None,
        out["notes"].as_str().unwrap(),
    )?;
    out["record"] = record.clone();
    out["schema"] = json!("fabric-measurements/1");
    out["wearer"] = json!({});
    out["taken_on"] = Value::Null;
    out["records"] = json!([record]);
    out["fused"] = json!({});
    Ok(out)
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct PhotoSectionOptions {
    level: String,
    same_wearer: bool,
    clothing_mm: f64,
    k: Option<f64>,
    k_sigma: Option<f64>,
    k_ratio: Option<f64>,
    k_ratio_sigma: Option<f64>,
    tape_girth_mm: Option<f64>,
    sigma_tape_mm: Option<f64>,
    name: Option<String>,
}

impl Default for PhotoSectionOptions {
    fn default() -> Self {
        Self {
            level: "waist".into(),
            same_wearer: false,
            clothing_mm: 0.0,
            k: None,
            k_sigma: None,
            k_ratio: None,
            k_ratio_sigma: None,
            tape_girth_mm: None,
            sigma_tape_mm: None,
            name: None,
        }
    }
}

fn photo_value(data: &Value, name: &str) -> Result<(f64, f64), Refusal> {
    let entry = &data["fused"][name];
    let value = entry["value_mm"]
        .as_f64()
        .ok_or_else(|| invalid(format!("no {name} record")))?;
    let sigma = entry["sigma_mm"]
        .as_f64()
        .ok_or_else(|| invalid(format!("{name} lacks uncertainty")))?;
    let sources = entry["sources"]
        .as_array()
        .ok_or_else(|| invalid(format!("{name} lacks provenance")))?;
    if entry["flags"]
        .as_array()
        .is_some_and(|flags| flags.iter().any(|f| f == "REMEASURE"))
    {
        return Err(invalid(format!(
            "{name}: conflicting evidence must be remeasured before deriving a girth"
        )));
    }
    let used: Vec<_> = sources.iter().filter(|s| s["used"] == true).collect();
    if used.is_empty() || used.iter().any(|s| s["provenance"] != "PHOTO_ESTIMATED") {
        return Err(invalid(format!(
            "{name}: section inputs require photo evidence, not chart/model values or a mixed-provenance estimate"
        )));
    }
    Ok((value, sigma))
}

fn source_document(text: &str) -> Result<Value, Refusal> {
    let value: Value = serde_json::from_str(text).map_err(|e| invalid(e.to_string()))?;
    if value["records"].as_array().is_some_and(|a| a.len() > 4096) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "section evidence exceeds 4096 records",
        ));
    }
    let errors = validate(&value);
    if !errors.is_empty() {
        return Err(invalid(format!(
            "invalid section measurement file: {}",
            errors.join("; ")
        )));
    }
    let reference = match &value["wearer"]["reference"] {
        Value::Null => false,
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_f64().is_some_and(|n| n != 0.0),
        Value::String(v) => !v.is_empty(),
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
    };
    if reference {
        return Err(invalid(
            "a reference wearer cannot supply real-wearer photo section evidence",
        ));
    }
    Ok(value)
}

/// Derive a girth or calibrate its shape coefficient from original photo measurement documents.
/// An empty depth source reuses the width document. Separate sources require explicit wearer
/// association; conflicting wearer metadata still refuses. Existing fused caches are never used.
/// Exact original texts and hashes survive alongside the selected, recomputed evidence.
///
/// # Errors
/// Malformed or oversized documents/options, absent or conflicting photo evidence, wearer
/// contradictions, or the ordinary section geometry/uncertainty guards. Calibration never emits
/// another measurement record and cannot be combined with shape-factor overrides.
pub fn photo_section(
    width_source: &str,
    depth_source: &str,
    options_source: &str,
) -> Result<Value, Refusal> {
    if width_source.len().saturating_add(depth_source.len()) > 16 * 1024 * 1024
        || options_source.len() > 256 * 1024
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "section sources exceed 16 MiB or options exceed 256 KiB",
        ));
    }
    let options: PhotoSectionOptions =
        serde_json::from_str(options_source).map_err(|e| invalid(e.to_string()))?;
    let supplied: Value =
        serde_json::from_str(options_source).map_err(|e| invalid(e.to_string()))?;
    for key in [
        "k",
        "k_sigma",
        "k_ratio",
        "k_ratio_sigma",
        "tape_girth_mm",
        "sigma_tape_mm",
        "name",
    ] {
        if supplied.get(key).is_some_and(Value::is_null) {
            return Err(invalid(format!("section option {key} cannot be null")));
        }
    }
    if options.tape_girth_mm.is_some() {
        if ["k", "k_sigma", "k_ratio", "k_ratio_sigma", "name"]
            .iter()
            .any(|key| supplied.get(key).is_some())
        {
            return Err(invalid(
                "shape-factor overrides and name do not apply when calibrating tape_girth_mm",
            ));
        }
    } else if options.sigma_tape_mm.is_some() {
        return Err(invalid("sigma_tape_mm requires tape_girth_mm"));
    }
    if !depth_source.is_empty() && !options.same_wearer {
        return Err(invalid(
            "confirm same_wearer before combining front and side photo evidence",
        ));
    }
    let width_document = source_document(width_source)?;
    let separate_depth = (!depth_source.is_empty())
        .then(|| source_document(depth_source))
        .transpose()?;
    let depth_document = separate_depth.as_ref().unwrap_or(&width_document);
    if separate_depth.as_ref().is_some_and(|d| {
        d["records"].as_array().unwrap().len() + width_document["records"].as_array().unwrap().len()
            > 4096
    }) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "combined section evidence exceeds 4096 records",
        ));
    }
    let mut wearer = width_document["wearer"].as_object().unwrap().clone();
    for (key, value) in depth_document["wearer"].as_object().unwrap() {
        if let Some(previous) = wearer.get(key)
            && !previous.is_null()
            && !value.is_null()
            && previous != value
        {
            return Err(invalid(format!(
                "section sources have contradictory wearer.{key} metadata"
            )));
        }
        if wearer.get(key).is_none_or(Value::is_null) {
            wearer.insert(key.clone(), value.clone());
        }
    }
    let width_data = fuse(&width_document, false)?;
    let depth_data = if separate_depth.is_some() {
        fuse(depth_document, false)?
    } else {
        width_data.clone()
    };
    let width_name = format!("{}_width", options.level);
    let depth_name = format!("{}_depth", options.level);
    let (width, sigma_width_mm) = photo_value(&width_data, &width_name)?;
    let (depth, sigma_depth_mm) = photo_value(&depth_data, &depth_name)?;
    let section = SectionOptions {
        level: &options.level,
        sigma_width_mm,
        sigma_depth_mm,
        k: options.k,
        k_sigma: options.k_sigma,
        k_ratio: options.k_ratio.unwrap_or(1.0),
        k_ratio_sigma: options.k_ratio_sigma.unwrap_or(0.0),
        clothing_mm: options.clothing_mm,
    };
    let mut out = if let Some(tape) = options.tape_girth_mm {
        calibrate_k(
            tape,
            width,
            depth,
            options.sigma_tape_mm.unwrap_or(10.0),
            &section,
        )?
    } else {
        section_measurement(width, depth, &section, options.name.as_deref())?
    };
    if out.get("records").is_some() {
        out["wearer"] = Value::Object(wearer);
        out["taken_on"] = match (
            width_document["taken_on"].as_str(),
            depth_document["taken_on"].as_str(),
        ) {
            (Some(a), Some(b)) => json!(a.min(b)),
            _ => Value::Null,
        };
    }
    out["section_inputs"] = json!({
        "width":width_data["fused"][&width_name],
        "depth":depth_data["fused"][&depth_name]
    });
    let source = |text: &str| json!({"text":text,"sha256":sha256_hex(text.as_bytes())});
    out["section_sources"] = json!({
        "width":source(width_source),
        "depth":(!depth_source.is_empty()).then(||source(depth_source)),
        "depth_uses_width_source":depth_source.is_empty(),
        "options":source(options_source)
    });
    Ok(out)
}
