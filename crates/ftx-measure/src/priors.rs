//! Weak public-data priors (spec 02 A.4). Tables are specification data, not generated oracle output.

use crate::{invalid, make_record, unit};
use ftx_core::{Refusal, py};
use serde_json::{Map, Value, json};
use std::sync::OnceLock;

pub(crate) const CDC: &str = "CDC 2000 stature-for-age statage.csv (03 §1.4c, VERIFIED)";
const OTT: &str = "Ottobre Design kids size chart, height-indexed rows 86–122 cm (ottobredesign.com/pages/size-charts; 03 §1.4b, VERIFIED)";
const C77: &str = "CPSC/UMTRI 1977 child anthropometry UM-HSRI-77-17 via NIST AnthroKids, combined sexes, age-band mean/SD scaled by stature ratio (03 §2.5, VERIFIED; 1970s sample)";
const C75: &str =
    "CPSC/UMTRI 1975 crotch height, data1975/73.csv via NIST AnthroKids (03 §2.5, VERIFIED)";
const WHO: &str = "WHO Child Growth Standards head-circumference-for-age medians, SD≈1.4 cm (03 §1.4d, VERIFIED); CDC 2000 cross-check";
const SECTION: &str = "derived: CPSC 1977 section shape (girth vs breadth) via k·Ramanujan inversion, scaled to the Ottobre girth (03 §2.5, §3.3)";
const JUDGEMENT: &str = "engineering judgement: no free source in 03";
const AGES: [f64; 7] = [24.5, 30.5, 36.5, 48.5, 60.5, 66.5, 72.5];

fn tables() -> &'static Value {
    static DATA: OnceLock<Value> = OnceLock::new();
    DATA.get_or_init(|| serde_json::from_str(include_str!("charts.json")).expect("chart data"))
}

fn interp(x: f64, xs: &[f64], ys: &[f64]) -> f64 {
    let i = xs
        .partition_point(|v| *v <= x)
        .saturating_sub(1)
        .min(xs.len() - 2);
    ys[i] + (x - xs[i]) / (xs[i + 1] - xs[i]) * (ys[i + 1] - ys[i])
}

fn keys(sex: Option<&str>) -> &'static [&'static str] {
    match sex
        .and_then(|s| s.chars().next())
        .map(|c| c.to_ascii_uppercase())
    {
        Some('M' | 'B') => &["M"],
        Some('F' | 'G') => &["F"],
        _ => &["M", "F"],
    }
}

/// CDC P50 stature and within-age standard deviation, in millimetres. Unknown sex averages both.
/// This is a population prior, not an individual measurement; ages extrapolate as in the reference.
pub fn cdc_stature(age: f64, sex: Option<&str>) -> Result<(f64, f64), Refusal> {
    Refusal::finite("age_months", age)?;
    let mut out = (0.0, 0.0);
    for key in keys(sex) {
        let rows = tables()["cdc"][key].as_array().expect("CDC rows");
        let median: Vec<f64> = rows.iter().map(|r| r[1].as_f64().unwrap()).collect();
        let sd: Vec<f64> = rows
            .iter()
            .map(|r| (r[2].as_f64().unwrap() - r[0].as_f64().unwrap()) / 3.29)
            .collect();
        out.0 += interp(age, &AGES, &median) * 10.0 / keys(sex).len() as f64;
        out.1 += interp(age, &AGES, &sd) * 10.0 / keys(sex).len() as f64;
    }
    if !out.0.is_finite() || !out.1.is_finite() || out.1 <= 0.0 {
        return Err(invalid("age outside the numerically usable chart range"));
    }
    Ok(out)
}

/// Sex-averaged CDC median age from stature, clamped to the tabulated 24.5–72.5 months.
pub fn age_from_stature(mm: f64) -> Result<f64, Refusal> {
    Refusal::finite("stature_mm", mm)?;
    let hs: Vec<f64> = AGES
        .iter()
        .map(|a| cdc_stature(*a, None).unwrap().0)
        .collect();
    Ok(interp(mm.clamp(hs[0], hs[6]), &hs, &AGES))
}

fn cpsc(age: f64, key: &str, column: usize) -> f64 {
    let row: Vec<f64> = tables()["cpsc"][key]
        .as_array()
        .expect("CPSC row")
        .iter()
        .map(|r| r[column].as_f64().unwrap())
        .collect();
    interp(age, &[33.0, 48.0, 60.0, 72.0], &row) * 10.0
}
fn ott(cm: f64, key: &str) -> f64 {
    let row: Vec<f64> = tables()["ottobre"][key]
        .as_array()
        .expect("Ottobre row")
        .iter()
        .map(|r| r.as_f64().unwrap())
        .collect();
    interp(cm, &[86.0, 92.0, 98.0, 104.0, 110.0, 116.0, 122.0], &row) * 10.0
}
fn ramanujan(a: f64, b: f64) -> f64 {
    let h = ((a - b) / (a + b)).powi(2);
    std::f64::consts::PI * (a + b) * (1.0 + 3.0 * h / (10.0 + (4.0 - 3.0 * h).sqrt()))
}
fn semi_axis(p: f64, a: f64) -> Result<f64, Refusal> {
    if !p.is_finite() || !a.is_finite() || a <= 0.0 || p <= 4.0 * a {
        return Err(invalid("perimeter too small for this semi-axis"));
    }
    let (mut lo, mut hi) = (1e-9, p / std::f64::consts::PI);
    for _ in 0..200 {
        let mid = (lo + hi) / 2.0;
        if ramanujan(a, mid) < p {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    Ok((lo + hi) / 2.0)
}

pub(crate) fn prior(
    name: &str,
    value: f64,
    sigma: f64,
    method: &str,
    source: &str,
    notes: &str,
) -> Result<Value, Refusal> {
    let mut out = make_record(name, value, sigma, "CHART_PRIOR", method, None, notes)?;
    let u = unit(name);
    let digits = if u == "deg" { 2 } else { 1 };
    out["source"] = json!(source);
    out[format!("value_{u}")] = json!(py::round(value, digits));
    out[format!("sigma_{u}")] = json!(py::round(sigma, digits));
    for (key, z) in [("p3_p97", 1.881), ("p1_p99", 2.326)] {
        out[format!("{key}_{u}")] = json!([
            py::round(value - z * sigma, digits),
            py::round(value + z * sigma, digits)
        ]);
    }
    Ok(out)
}

struct Priors {
    out: Map<String, Value>,
    base: String,
}
impl Priors {
    fn add(
        &mut self,
        name: &str,
        value: f64,
        sigma: f64,
        method: &str,
        source: &str,
        notes: &[&str],
    ) -> Result<(), Refusal> {
        let mut parts = notes.to_vec();
        parts.push(&self.base);
        let notes = parts
            .into_iter()
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join("; ");
        self.out.insert(
            name.into(),
            prior(name, value, sigma, method, source, &notes)?,
        );
        Ok(())
    }
    fn value(&self, name: &str) -> f64 {
        self.out[name]["value_mm"].as_f64().unwrap()
    }
    fn sigma(&self, name: &str) -> f64 {
        self.out[name]["sigma_mm"].as_f64().unwrap()
    }
}

/// All 35 priors when age is given, 34 otherwise (stature is then the conditioning input).
/// Coverage is deliberately limited to 800–1280 mm. Every result remains `CHART_PRIOR`.
pub fn priors_for(
    stature_mm: f64,
    age_months: Option<f64>,
    sex: Option<&str>,
) -> Result<Value, Refusal> {
    Refusal::finite("stature_mm", stature_mm)?;
    if !(800.0..=1280.0).contains(&stature_mm) {
        return Err(invalid(
            "chart priors cover stature 80–128 cm (toddlers and young children); measure everything else",
        ));
    }
    let cm = stature_mm / 10.0;
    let age = age_months.unwrap_or(age_from_stature(stature_mm)?);
    Refusal::finite("age_months", age)?;
    let mut notes = Vec::new();
    if age_months.is_none() {
        notes.push(format!("age inferred from CDC P50 stature: {age:.0} mo"));
    }
    if !(18.0..=84.0).contains(&age) {
        notes.push("age outside 18–84 mo: priors extrapolated (UNCERTAIN)".into());
    }
    if !(86.0..=122.0).contains(&cm) {
        notes.push("stature outside Ottobre rows 86–122 cm: extrapolated (UNCERTAIN)".into());
    }
    let mut p = Priors {
        out: Map::new(),
        base: notes.join("; "),
    };
    if age_months.is_some() {
        let (v, s) = cdc_stature(age, sex)?;
        p.add(
            "stature",
            v,
            s,
            &format!("CDC P50 at {age:.0} mo"),
            CDC,
            &["only used when stature is not measured"],
        )?;
    }
    let om = format!("Ottobre row interpolated at {cm:.1} cm");
    for (name, sigma) in [
        ("chest", 30.0),
        ("waist", 35.0),
        ("hip", 35.0),
        ("arm_length", 13.0),
        ("outseam", 30.0),
        ("back_neck_to_waist", 15.0),
    ] {
        p.add(
            name,
            ott(cm, name),
            sigma,
            &om,
            OTT,
            &["σ = within-age SD per 03 §0.2"],
        )?;
    }
    p.add(
        "shoulder_length",
        ott(cm, "shoulder_length"),
        6.0,
        &om,
        OTT,
        &["UNCERTAIN σ: Ottobre gives no SD; 6 mm by analogy with biacromial CV≈6%"],
    )?;
    p.add(
        "seat_over_nappy",
        p.value("hip") + 50.0,
        35.0_f64.hypot(20.0),
        "hip prior + 5 cm nappy bulk",
        JUDGEMENT,
        &["UNCERTAIN placeholder: nappy bulk is not tabulated; always tape it over the nappy"],
    )?;
    let ac = age.clamp(33.5, 69.5);
    let rc = interp(ac, &[33.5, 69.5], &[37.7 / 93.1, 50.6 / 113.3]);
    let crotch = rc * cm * 10.0;
    let check = if cm <= 95.0 {
        format!(
            "cross-check Ottobre toddler inside leg {:.1} cm",
            interp(cm, &[86.0, 92.0], &[33.5, 37.0])
        )
    } else {
        String::new()
    };
    for name in ["crotch_height", "inseam"] {
        p.add(
            name,
            crotch,
            30.0,
            &format!("crotch/stature ratio {rc:.3} at {ac:.0} mo"),
            C75,
            &["σ = 3 cm per 03 §0.2", &check],
        )?;
    }
    let waist_h = p.value("outseam") - 10.0;
    p.add(
        "waist_height",
        waist_h,
        30.0,
        "Ottobre outside leg − 1 cm (tape follows the hip)",
        OTT,
        &["UNCERTAIN offset (surface over hip vs vertical)"],
    )?;
    let rise = waist_h - crotch + 15.0;
    p.add("body_rise",rise,25.0,"waist height − crotch height + 1.5 cm seated (03 §1.1, §4.4)",&format!("derived from {OTT} and {C75}"),&["UNCERTAIN: ribbon-level (Ottobre) waist; the CPSC navel-level derivation gives ≈11–12 cm standing at 3 y (03 §2.5) — the waist level is the difference"])?;
    p.add(
        "total_crotch_length",
        2.3 * rise,
        40.0,
        "2.3 × body rise",
        JUDGEMENT,
        &["UNCERTAIN placeholder (no free child source); tape it (06 Q3)"],
    )?;
    p.add(
        "knee_height",
        0.62 * crotch,
        20.0,
        "0.62 × crotch height",
        JUDGEMENT,
        &["UNCERTAIN placeholder"],
    )?;
    let mut head = 0.0;
    for k in keys(sex) {
        let row = if *k == "M" {
            [48.3, 49.5, 50.2, 50.7]
        } else {
            [47.2, 48.5, 49.3, 49.9]
        };
        head += interp(age, &[24.0, 36.0, 48.0, 60.0], &row) * 10.0 / keys(sex).len() as f64;
    }
    let both = if keys(sex).len() == 2 {
        ", sexes averaged"
    } else {
        ""
    };
    let extra = if !(24.0..=60.0).contains(&age) {
        "extrapolated beyond 24–60 mo (UNCERTAIN)"
    } else {
        ""
    };
    p.add(
        "head",
        head,
        15.0,
        &format!("WHO median at {age:.0} mo{both}"),
        WHO,
        &["σ = 1.5 cm per 03 §0.2", extra],
    )?;
    let agec = age.clamp(33.0, 72.0);
    let ratio = stature_mm / cpsc(agec, "stature", 0);
    let scale = ratio.sqrt();
    let age_note = if agec == age {
        String::new()
    } else {
        format!("CPSC band centres span 33–72 mo; nearest band used at {age:.0} mo")
    };
    let method = format!("CPSC 1977 at {agec:.0} mo × (stature ratio {ratio:.3})");
    let elastic = "girth scaled by ratio^0.5 (Ottobre steps imply ≈0.55; UNCERTAIN)";
    p.add(
        "neck_base",
        cpsc(agec, "neck_mid", 0) * scale + 15.0,
        13.0_f64.hypot(8.0),
        &format!("{method}: mid-neck #32 + 1.5 cm"),
        C77,
        &[
            "UNCERTAIN: neck base runs larger than CPSC mid-neck girth; +1.5 cm is judgement",
            elastic,
            &age_note,
        ],
    )?;
    let ua = cpsc(agec, "upper_arm", 0) * scale;
    p.add(
        "upper_arm",
        ua,
        14.0,
        &format!("{method}: #39"),
        C77,
        &[elastic, &age_note],
    )?;
    for (name, src) in [
        ("thigh", "upper_thigh"),
        ("calf", "calf"),
        ("ankle", "ankle"),
    ] {
        p.add(
            name,
            cpsc(agec, src, 0) * scale,
            cpsc(agec, src, 1) * scale,
            &format!("{method}: {src}"),
            C77,
            &[elastic, &age_note],
        )?;
    }
    p.add(
        "across_back",
        cpsc(agec, "biacromial", 0) * ratio + 10.0,
        (cpsc(agec, "biacromial", 1) * ratio).hypot(8.0),
        &format!("{method}: biacromial #36 + 1 cm surface allowance"),
        C77,
        &[
            "UNCERTAIN: surface across back vs bony breadth offset is judgement",
            &age_note,
        ],
    )?;
    for (name, v, s, m) in [
        ("wrist", 0.70 * ua, 8.0, "0.70 × upper-arm prior"),
        ("hand_pass", 1.30 * 0.70 * ua, 10.0, "1.30 × wrist prior"),
        ("knee", 1.03 * p.value("calf"), 20.0, "1.03 × calf prior"),
        (
            "foot_pass",
            1.40 * p.value("ankle"),
            15.0,
            "1.40 × ankle prior",
        ),
    ] {
        p.add(name, v, s, m, JUDGEMENT, &["UNCERTAIN placeholder"])?;
    }
    p.add(
        "shoulder_slope_deg",
        18.0,
        5.0,
        "placeholder",
        JUDGEMENT,
        &["UNCERTAIN placeholder: measure from the P3 back photo (06 Q3)"],
    )?;
    p.add(
        "armscye_depth",
        0.13 * cm * 10.0,
        15.0,
        "0.13 × stature",
        JUDGEMENT,
        &["UNCERTAIN placeholder; drafted from chest and stature (03 §1.2)"],
    )?;
    let sn = "UNCERTAIN: section shape from 1970s CPSC breadths and adult k (03 §3.3)";
    for (level, g, b, k) in [
        ("chest", "chest_axilla", "chest_breadth", 1.07),
        ("waist", "waist_natural", "waist_breadth_navel", 1.06),
        ("hip", "hip_buttocks", "hip_breadth_trochanter", 1.09),
    ] {
        let cg = cpsc(agec, g, 0);
        let cb = cpsc(agec, b, 0);
        let d = 2.0 * semi_axis(cg / k, cb / 2.0)?;
        let s = p.value(level) / cg;
        p.add(
            &format!("{level}_width"),
            cb * s,
            cpsc(agec, b, 1) * s,
            &format!("CPSC {b} × {s:.3} (Ottobre/CPSC girth ratio)"),
            SECTION,
            &[sn, &age_note],
        )?;
        p.add(
            &format!("{level}_depth"),
            d * s,
            0.6 * p.sigma(level),
            &format!("inverted from CPSC {g} and {b}, k={k}"),
            SECTION,
            &[sn, "σ = 0.6 × girth σ", &age_note],
        )?;
    }
    let belly =
        2.0 * semi_axis(
            cpsc(agec, "waist_navel", 0) / 1.06,
            cpsc(agec, "waist_breadth_navel", 0) / 2.0,
        )? * p.value("waist")
            / cpsc(agec, "waist_natural", 0);
    p.add(
        "belly_depth",
        belly,
        0.6 * p.sigma("waist"),
        "navel-level depth inverted from CPSC #66/#67, scaled like waist",
        SECTION,
        &[sn, "measure it from the P2 side photo (06 Q3)", &age_note],
    )?;
    Ok(Value::Object(p.out))
}
