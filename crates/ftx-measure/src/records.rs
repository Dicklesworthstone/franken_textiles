//! Record construction and validation (spec 02 A.3).

use ftx_core::Refusal;
use serde_json::{Value, json};

use crate::{PROVENANCES, canonical_name, definition, invalid, unit};

/// Gregorian day ordinal (0001-01-01 = 1), or `None` for a non-ISO/invalid date.
/// No clock is consulted, so staleness checks can be deterministic.
#[must_use]
pub fn date_day(date: &str) -> Option<i64> {
    let b = date.as_bytes();
    if b.len() != 10
        || b[4] != b'-'
        || b[7] != b'-'
        || b.iter()
            .enumerate()
            .any(|(i, c)| i != 4 && i != 7 && !c.is_ascii_digit())
    {
        return None;
    }
    let y: i64 = date[..4].parse().ok()?;
    let m: usize = date[5..7].parse().ok()?;
    let d: i64 = date[8..].parse().ok()?;
    if y == 0 || !(1..=12).contains(&m) {
        return None;
    }
    let leap = y % 4 == 0 && (y % 100 != 0 || y % 400 == 0);
    let months = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1..=months[m - 1]).contains(&d) {
        return None;
    }
    let before = y - 1;
    Some(
        before * 365 + before / 4 - before / 100
            + before / 400
            + months[..m - 1].iter().sum::<i64>()
            + d,
    )
}

/// Parses an explicitly unit-labelled length, or an angle with optional degree suffix.
/// Rejects exponents, NaNs, bare lengths, wrong dimensions and overflowing conversions.
pub fn parse_value(text: &str, measurement_unit: &str) -> Result<f64, Refusal> {
    let s = text.trim();
    let bytes = s.as_bytes();
    let mut n = usize::from(bytes.first().is_some_and(|c| *c == b'+' || *c == b'-'));
    let start = n;
    while n < bytes.len() && bytes[n].is_ascii_digit() {
        n += 1;
    }
    let mut valid = n > start;
    if n < bytes.len() && bytes[n] == b'.' {
        n += 1;
        let frac = n;
        while n < bytes.len() && bytes[n].is_ascii_digit() {
            n += 1;
        }
        valid &= n > frac;
    }
    let malformed = || {
        invalid(format!(
            "cannot read '{text}'; write e.g. 55cm, 21.5in, 550mm"
        ))
    };
    if !valid {
        return Err(malformed());
    }
    let suffix = s[n..].trim().to_lowercase();
    let factor = match (measurement_unit, suffix.as_str()) {
        ("deg", "" | "deg" | "°") => 1.0,
        ("deg", "mm" | "cm" | "m" | "in" | "inch" | "inches" | "\"") => {
            return Err(invalid(format!("'{text}': an angle, give degrees")));
        }
        ("mm", "mm") => 1.0,
        ("mm", "cm") => 10.0,
        ("mm", "m") => 1000.0,
        ("mm", "in" | "inch" | "inches" | "\"") => 25.4,
        ("mm", "") => {
            return Err(invalid(format!(
                "'{text}': give a unit (mm, cm, in) — tape numbers are ambiguous without one"
            )));
        }
        _ => return Err(malformed()),
    };
    let value = s[..n].parse::<f64>().map_err(|_| malformed())? * factor;
    Refusal::finite("measurement value", value)
}

/// Makes a canonical record. Unlike the reference's unchecked constructor, refuses invalid numbers.
pub fn make_record(
    name: &str,
    value: f64,
    sigma: f64,
    provenance: &str,
    method: &str,
    definition_id: Option<&str>,
    notes: &str,
) -> Result<Value, Refusal> {
    let name = canonical_name(name);
    if name.is_empty() {
        return Err(invalid("measurement needs a name"));
    }
    Refusal::finite("measurement value", value)?;
    Refusal::finite("measurement sigma", sigma)?;
    if sigma <= 0.0 {
        return Err(invalid("measurement sigma must be > 0"));
    }
    if !PROVENANCES.contains(&provenance) {
        return Err(invalid(format!(
            "provenance must be one of {}",
            PROVENANCES.join("|")
        )));
    }
    let u = unit(&name);
    let did = definition_id.or_else(|| definition(&name).and_then(|d| d["definition_id"].as_str()));
    let mut record = json!({"name": name, "definition_id": did,
        "provenance": provenance, "method": method, "notes": notes});
    record[format!("value_{u}")] = json!(value);
    record[format!("sigma_{u}")] = json!(sigma);
    Ok(record)
}

/// Ordered structural errors; booleans are not accepted as numbers. Fused values are recomputed.
#[must_use]
pub fn validate(data: &Value) -> Vec<String> {
    let mut errors = Vec::new();
    if !data.is_object() {
        return vec!["top level must be a JSON object".into()];
    }
    if data["schema"] != "fabric-measurements/1" {
        errors.push("schema must be 'fabric-measurements/1'".into());
    }
    if !data["wearer"].is_object() {
        errors.push("wearer must be an object".into());
    } else {
        for key in ["age_months", "stature_mm"] {
            let v = &data["wearer"][key];
            if !v.is_null() && !v.as_f64().is_some_and(f64::is_finite) {
                errors.push(format!("wearer.{key} must be a number or null"));
            }
        }
    }
    if !data["taken_on"].is_null() && data["taken_on"].as_str().and_then(date_day).is_none() {
        errors.push("taken_on must be YYYY-MM-DD or null".into());
    }
    if let Some(records) = data["records"].as_array() {
        for (i, r) in records.iter().enumerate() {
            let Some(name) = r["name"]
                .as_str()
                .map(canonical_name)
                .filter(|n| !n.is_empty())
            else {
                errors.push(format!("records[{i}]: needs a name"));
                continue;
            };
            let u = unit(&name);
            let vk = format!("value_{u}");
            let sk = format!("sigma_{u}");
            let prefix = format!("records[{i}] {name}");
            if r.get(&vk).is_none() {
                let hint = if u == "deg" && r.get("value_mm").is_some() {
                    format!(" ({name} is an angle: use value_deg/sigma_deg)")
                } else {
                    String::new()
                };
                errors.push(format!("{prefix}: missing {vk}{hint}"));
                continue;
            }
            if !r[&vk].as_f64().is_some_and(f64::is_finite) {
                errors.push(format!("{prefix}: {vk} must be a finite number"));
            }
            if !r[&sk].as_f64().is_some_and(|s| s.is_finite() && s > 0.0) {
                errors.push(format!("{prefix}: {sk} must be > 0"));
            }
            if !r["provenance"]
                .as_str()
                .is_some_and(|p| PROVENANCES.contains(&p))
            {
                errors.push(format!(
                    "{prefix}: provenance must be one of {}",
                    PROVENANCES.join("|")
                ));
            }
        }
    } else {
        errors.push("records must be a list".into());
    }
    if data.get("fused").is_some_and(|v| !v.is_object()) {
        errors.push("fused must be an object".into());
    }
    errors
}
