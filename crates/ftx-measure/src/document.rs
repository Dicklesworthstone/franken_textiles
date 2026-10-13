//! Whole-document evidence fusion and the explicitly synthetic reference wearer (A.6.4, A.8).

use crate::{
    NAMES, age_from_stature, canonical_name, cdc_stature, definition, fuse_one, invalid,
    make_record, normalize_record, priors_for, unit, validate,
};
use ftx_core::{Refusal, py};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};

/// Recomputes all fused values from original records; never trusts a stale `fused` cache.
/// Disabled priors still supply sanity bounds to measured values. Explicit user priors remain
/// available, including outside the built-in chart's stature range.
pub fn fuse(data: &Value, use_priors: bool) -> Result<Value, Refusal> {
    let errors = validate(data);
    if !errors.is_empty() {
        return Err(invalid(format!(
            "invalid measurement file: {}",
            errors.join("; ")
        )));
    }
    let mut grouped: BTreeMap<String, Vec<Value>> = BTreeMap::new();
    let mut excluded: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut normalized = BTreeSet::new();
    for record in data["records"].as_array().unwrap() {
        let (r, status) = normalize_record(record)?;
        let name = r["name"].as_str().unwrap().to_string();
        if status == "excluded" {
            excluded
                .entry(name)
                .or_default()
                .push(r["note"].as_str().unwrap().into());
            continue;
        }
        if status == "normalized" {
            normalized.insert(name.clone());
        }
        grouped.entry(name).or_default().push(r);
    }
    let wearer = &data["wearer"];
    let age = wearer["age_months"].as_f64();
    let sex = wearer["sex"].as_str();
    let heights = grouped.get("stature").map(Vec::as_slice).unwrap_or(&[]);
    let measured: Vec<Value> = heights
        .iter()
        .filter(|r| r["provenance"] != "CHART_PRIOR")
        .cloned()
        .collect();
    let (stature, stature_source) = if !measured.is_empty() {
        (
            fuse_one("stature", &measured, None, false)?.and_then(|r| r["value_mm"].as_f64()),
            "fused stature records",
        )
    } else if let Some(p) = heights.iter().find(|r| r["provenance"] == "CHART_PRIOR") {
        (p["value_mm"].as_f64(), "stature CHART_PRIOR record")
    } else if let Some(h) = wearer["stature_mm"].as_f64() {
        (Some(h), "wearer.stature_mm")
    } else if let Some(age) = age {
        (
            Some(cdc_stature(age, sex)?.0),
            "CDC P50 for age (stature not measured)",
        )
    } else {
        (None, "none")
    };
    let (priors, notes) = match stature {
        Some(h) => match priors_for(h, age, sex) {
            Ok(p) => (p, Vec::<String>::new()),
            Err(e) => (json!({}), vec![e.message]),
        },
        None => (
            json!({}),
            vec!["no stature or age: no chart priors or sanity bounds".into()],
        ),
    };
    let names = NAMES
        .into_iter()
        .map(str::to_string)
        .chain(grouped.keys().filter(|n| definition(n).is_none()).cloned());
    let mut fused = Map::new();
    for name in names {
        let records = grouped.get(&name).map(Vec::as_slice).unwrap_or(&[]);
        let mut evidence: Vec<Value> = records
            .iter()
            .filter(|r| r["provenance"] != "CHART_PRIOR")
            .cloned()
            .collect();
        let sk = format!("sigma_{}", unit(&name));
        let user_prior = records
            .iter()
            .filter(|r| r["provenance"] == "CHART_PRIOR")
            .min_by(|a, b| {
                a[&sk]
                    .as_f64()
                    .unwrap()
                    .total_cmp(&b[&sk].as_f64().unwrap())
            });
        // A numeric wearer height is explicitly supplied evidence, even without an age or charts.
        if name == "stature"
            && evidence.is_empty()
            && user_prior.is_none()
            && let Some(h) = wearer["stature_mm"].as_f64()
        {
            let mut r = make_record(
                "stature",
                h,
                10.0,
                wearer["stature_provenance"]
                    .as_str()
                    .unwrap_or("MEASURED_TAPE"),
                "wearer.stature_mm",
                None,
                "",
            )?;
            r["note"] = json!("from the wearer block");
            evidence.push(r);
        }
        let chart = priors
            .get(&name)
            .filter(|_| use_priors || !evidence.is_empty())
            .map(|p| {
                let mut p = p.clone();
                p["auto_prior"] = json!(true);
                p
            });
        let chosen = user_prior.or(chart.as_ref());
        let Some(mut entry) =
            fuse_one(&name, &evidence, chosen, use_priors || user_prior.is_some())?
        else {
            continue;
        };
        let mut flags: BTreeSet<String> = entry["flags"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().into())
            .collect();
        if normalized.contains(&name) {
            flags.insert("NORMALIZED_DEFINITION".into());
        }
        if let Some(ex) = excluded.get(&name) {
            flags.insert("UNNORMALIZED_EXCLUDED".into());
            let old = entry["notes"].as_str().unwrap_or("");
            let mut notes = Vec::new();
            if !old.is_empty() {
                notes.push(old.to_string());
            }
            notes.extend(ex.clone());
            entry["notes"] = json!(notes.join("; "));
        }
        if definition(&name).is_none() {
            flags.insert("NOT_IN_CATALOGUE".into());
        }
        entry["flags"] = json!(flags);
        fused.insert(name, entry);
    }
    let mut out = data.clone();
    out["fused"] = Value::Object(fused);
    out["fusion"] = json!({"stature_for_priors_mm":stature.map(|h|py::round(h,1)),"stature_source":stature_source,
        "priors_used":use_priors&&priors.as_object().is_some_and(|p|!p.is_empty()),"z_refuse":3.0,"z_warn":2.0,"notes":notes});
    if !excluded.is_empty() {
        out["fusion"]["excluded"] = json!(excluded);
    }
    Ok(out)
}

/// The (value, one-sigma uncertainty) pair in a fused document; aliases are accepted.
#[must_use]
pub fn fused_value(data: &Value, name: &str) -> Option<(f64, f64)> {
    let name = canonical_name(name);
    let u = unit(&name);
    let r = &data["fused"][&name];
    Some((
        r[format!("value_{u}")].as_f64()?,
        r[format!("sigma_{u}")].as_f64()?,
    ))
}

/// An explicitly fictional reference wearer made solely from public priors, never tape evidence.
/// Readiness may use `READY_REFERENCE`, but still reports what a real wearer would need measured.
pub fn reference_wearer(
    stature_mm: f64,
    age_months: Option<f64>,
    sex: Option<&str>,
    label: Option<&str>,
) -> Result<Value, Refusal> {
    let mut priors = priors_for(stature_mm, age_months, sex)?;
    let age = age_months.unwrap_or(age_from_stature(stature_mm)?);
    let sigma = cdc_stature(age, sex)?.1;
    priors["stature"] = crate::priors::prior(
        "stature",
        stature_mm,
        sigma,
        "conditioning stature of the reference wearer (by definition)",
        crate::priors::CDC,
        "σ = CDC within-age SD; the value is the definition",
    )?;
    let records: Vec<Value> = NAMES.iter().map(|n| priors[n].clone()).collect();
    let mut data = json!({"schema":"fabric-measurements/1","wearer":{"label":label.map(str::to_string).unwrap_or_else(||format!("reference toddler {} cm",stature_mm/10.0)),"age_months":age_months,"stature_mm":stature_mm,"reference":true},
        "taken_on":null,"records":records,"fused":{},"notes":"Reference wearer built only from public priors (03 §1.4, §2.5); not a real child."});
    if let Some(sex) = sex {
        data["wearer"]["sex"] = json!(sex);
    }
    fuse(&data, true)
}
