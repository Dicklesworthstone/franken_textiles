//! Definition normalization and correlated-repeat-aware fusion (spec 02 A.6).

use std::collections::BTreeSet;

use ftx_core::{Refusal, py};
use serde_json::{Value, json};

use crate::{PROVENANCES, canonical_name, definition, invalid, unit};

fn number(record: &Value, key: &str) -> Result<f64, Refusal> {
    record[key]
        .as_f64()
        .filter(|v| v.is_finite())
        .ok_or_else(|| invalid(format!("{key} must be a finite number")))
}

fn checked_record(record: &Value) -> Result<(String, &'static str, f64, f64), Refusal> {
    let name = record["name"]
        .as_str()
        .map(canonical_name)
        .filter(|n| !n.is_empty())
        .ok_or_else(|| invalid("measurement needs a name"))?;
    let u = unit(&name);
    let value = number(record, &format!("value_{u}"))?;
    let sigma = number(record, &format!("sigma_{u}"))?;
    if sigma <= 0.0 {
        return Err(invalid("measurement sigma must be > 0"));
    }
    if !record["provenance"]
        .as_str()
        .is_some_and(|p| PROVENANCES.contains(&p))
    {
        return Err(invalid("unknown measurement provenance"));
    }
    Ok((name, u, value, sigma))
}

/// Normalizes one record and returns the copy plus `ok`, `normalized`, or `excluded`.
/// The latter must never enter a fusion; its `note` explains the incompatible definition.
pub fn normalize_record(record: &Value) -> Result<(Value, &'static str), Refusal> {
    let (name, u, value, sigma) = checked_record(record)?;
    let mut out = record.clone();
    out["name"] = json!(name);
    let Some(def) = definition(&name) else {
        return Ok((out, "ok"));
    };
    let Some(did) = record["definition_id"].as_str().filter(|d| !d.is_empty()) else {
        return Ok((out, "ok"));
    };
    if def["definition_id"] == did
        || def["equivalent_ids"]
            .as_array()
            .is_some_and(|ids| ids.iter().any(|id| id == did))
    {
        return Ok((out, "ok"));
    }
    let target = def["definition_id"].as_str().unwrap_or("");
    if let Some(hook) = def["normalization_hooks"].get(did) {
        let offset = hook[0].as_f64().unwrap_or(0.0);
        let extra = hook[1].as_f64().unwrap_or(0.0);
        let value = Refusal::finite("normalized value", value + offset)?;
        let sigma = Refusal::finite("normalized sigma", sigma.hypot(extra))?;
        out[format!("value_{u}")] = json!(value);
        out[format!("sigma_{u}")] = json!(sigma);
        out["note"] = json!(format!(
            "{did} → {target}: {offset:+.0} mm, σ+{extra:.0} ({})",
            hook[2].as_str().unwrap_or("")
        ));
        Ok((out, "normalized"))
    } else {
        out["note"] = json!(format!(
            "definition {did} has no normalization to {target}; not fused"
        ));
        Ok((out, "excluded"))
    }
}

#[derive(Clone)]
struct Observation {
    record: Value,
    value: f64,
    sigma: f64,
    n: usize,
}

impl Observation {
    fn read(record: &Value, name: &str) -> Result<Self, Refusal> {
        let (actual, _, value, sigma) = checked_record(record)?;
        if actual != name {
            return Err(invalid(format!("expected {name}, got {actual}")));
        }
        Ok(Self {
            record: record.clone(),
            value,
            sigma,
            n: 1,
        })
    }

    fn provenance(&self) -> &str {
        self.record["provenance"].as_str().unwrap_or("")
    }
    fn method(&self) -> &str {
        self.record["method"].as_str().unwrap_or("")
    }

    fn source(&self, u: &str, weight: f64, used: bool) -> Value {
        let digits = if u == "deg" { 2 } else { 1 };
        let mut src = json!({"provenance": self.provenance(), "method": self.method(),
            "definition_id": self.record["definition_id"],
            "weight": py::round(weight, 4), "used": used});
        src[format!("value_{u}")] = json!(py::round(self.value, digits));
        src[format!("sigma_{u}")] = json!(py::round(self.sigma, digits));
        if self.n > 1 {
            src["n_repeats"] = json!(self.n);
        }
        if let Some(note) = self.record["note"].as_str().filter(|s| !s.is_empty()) {
            src["note"] = json!(note);
        }
        if self.record["auto_prior"] == true {
            src["auto_prior"] = json!(true);
        }
        src
    }
}

fn median(mut values: Vec<f64>) -> f64 {
    values.sort_by(f64::total_cmp);
    let n = values.len();
    if n % 2 == 1 {
        values[n / 2]
    } else {
        values[n / 2 - 1] / 2.0 + values[n / 2] / 2.0
    }
}

fn collapse(
    records: &[Value],
    name: &str,
    flags: &mut BTreeSet<String>,
) -> Result<Vec<Observation>, Refusal> {
    let mut groups: Vec<Vec<Observation>> = Vec::new();
    for record in records {
        let obs = Observation::read(record, name)?;
        if let Some(group) = groups
            .iter_mut()
            .find(|g| g[0].provenance() == obs.provenance() && g[0].method() == obs.method())
        {
            group.push(obs);
        } else {
            groups.push(vec![obs]);
        }
    }
    let mut result = Vec::new();
    for group in groups {
        let mut obs = group[0].clone();
        if group.len() > 1 {
            obs.n = group.len();
            let mean = group.iter().map(|o| o.value / obs.n as f64).sum::<f64>();
            let spread = (group.iter().map(|o| (o.value - mean).powi(2)).sum::<f64>()
                / (obs.n - 1) as f64)
                .sqrt();
            Refusal::finite("repeat spread", spread)?;
            let stated = median(group.iter().map(|o| o.sigma).collect());
            obs.value = median(group.iter().map(|o| o.value).collect());
            obs.sigma = stated.max(spread);
            let old = obs.record["note"].as_str().unwrap_or("");
            let separator = if old.is_empty() { "" } else { "; " };
            obs.record["note"] = json!(format!(
                "{old}{separator}{} repeats: median, spread {spread:.1}",
                obs.n
            ));
            flags.insert("REPEATS_COLLAPSED".into());
            if spread > 1.5 * stated {
                flags.insert("REPEATS_SPREAD".into());
            }
        }
        result.push(obs);
    }
    Ok(result)
}

fn weighted(observations: &[Observation]) -> Result<(f64, f64, Vec<f64>), Refusal> {
    // Scale by the best sigma: avoids squaring a tiny uncertainty into zero.
    let scale = observations
        .iter()
        .map(|o| o.sigma)
        .fold(f64::INFINITY, f64::min);
    let mut weights: Vec<f64> = observations
        .iter()
        .map(|o| (scale / o.sigma).powi(2))
        .collect();
    let total: f64 = weights.iter().sum();
    for w in &mut weights {
        *w /= total;
    }
    let value = observations
        .iter()
        .zip(&weights)
        .map(|(o, w)| o.value * w)
        .sum::<f64>();
    let sigma = scale / total.sqrt();
    Refusal::finite("fused value", value)?;
    Refusal::finite("fused sigma", sigma)?;
    if sigma == 0.0 {
        return Err(invalid("fused sigma underflow"));
    }
    Ok((value, sigma, weights))
}

fn entry(
    u: &str,
    value: f64,
    sigma: f64,
    provenance: &str,
    sources: Vec<Value>,
    flags: &BTreeSet<String>,
    bounds: Option<[f64; 2]>,
) -> Value {
    let digits = if u == "deg" { 2 } else { 1 };
    let mut out = json!({"unit": u, "primary_provenance": provenance,
        "sources": sources, "flags": flags});
    out[format!("value_{u}")] = json!(py::round(value, digits));
    out[format!("sigma_{u}")] = json!(py::round(sigma, digits));
    if let Some(bounds) = bounds {
        out[format!("p1_p99_{u}")] = json!(bounds);
    }
    out
}

/// Fuses normalized observations of one measurement, with an optional supplied prior (A.6.3).
/// Repeats sharing provenance and method are correlated; incompatible methods at z > 3 are not
/// averaged. `use_prior = false` still uses the prior for sanity bounds, never for the value.
/// Empty observations and no prior return `None`. Call [`normalize_record`] before this function.
pub fn fuse_one(
    name: &str,
    records: &[Value],
    prior: Option<&Value>,
    use_prior: bool,
) -> Result<Option<Value>, Refusal> {
    let name = canonical_name(name);
    let u = unit(&name);
    let mut flags = BTreeSet::new();
    let mut observations = collapse(records, &name, &mut flags)?;
    let prior = prior.map(|p| Observation::read(p, &name)).transpose()?;
    if prior
        .as_ref()
        .is_some_and(|p| p.provenance() != "CHART_PRIOR")
    {
        return Err(invalid("a prior must have CHART_PRIOR provenance"));
    }
    let bounds = if let Some(p) = &prior {
        let key = format!("p1_p99_{u}");
        let b = if let Some(b) = p.record.get(&key) {
            let a = b
                .as_array()
                .filter(|a| a.len() == 2)
                .ok_or_else(|| invalid("prior bounds must contain two finite numbers"))?;
            [
                a[0].as_f64().unwrap_or(f64::NAN),
                a[1].as_f64().unwrap_or(f64::NAN),
            ]
        } else {
            [p.value - 2.326 * p.sigma, p.value + 2.326 * p.sigma]
        };
        if !b.iter().all(|v| v.is_finite()) || b[0] > b[1] {
            return Err(invalid("prior bounds must be finite and ordered"));
        }
        Some(b)
    } else {
        None
    };
    if observations.is_empty() {
        return Ok(prior.map(|p| {
            flags.insert("PRIOR_ONLY".into());
            entry(
                u,
                p.value,
                p.sigma,
                "CHART_PRIOR",
                vec![p.source(u, 1.0, true)],
                &flags,
                bounds,
            )
        }));
    }
    let mut z_max = 0.0;
    let mut pair = (0, 0);
    for i in 0..observations.len() {
        for j in i + 1..observations.len() {
            let a = &observations[i];
            let b = &observations[j];
            let z = (a.value - b.value).abs() / a.sigma.hypot(b.sigma);
            Refusal::finite("source disagreement", z)?;
            if z > z_max {
                z_max = z;
                pair = (i, j);
            }
        }
    }
    let multiple = observations.len() >= 2;
    if z_max > 3.0 {
        let best = observations
            .iter()
            .enumerate()
            .min_by(|a, b| a.1.sigma.total_cmp(&b.1.sigma))
            .map(|(i, _)| i)
            .unwrap_or(0);
        flags.insert("REMEASURE".into());
        let a = &observations[pair.0];
        let b = &observations[pair.1];
        let sources = observations
            .iter()
            .enumerate()
            .map(|(i, o)| o.source(u, if i == best { 1.0 } else { 0.0 }, i == best))
            .collect();
        let chosen = &observations[best];
        let mut out = entry(
            u,
            chosen.value,
            chosen.sigma,
            chosen.provenance(),
            sources,
            &flags,
            bounds,
        );
        out["z_max"] = json!(py::round(z_max, 2));
        out["notes"] = json!(format!(
            "not fused: {} {:.1}±{:.1} vs {} {:.1}±{:.1}, z={z_max:.2} > 3 — re-measure this one",
            a.provenance(),
            a.value,
            a.sigma,
            b.provenance(),
            b.value,
            b.sigma
        ));
        return Ok(Some(out));
    }
    let mut notes = Vec::new();
    if z_max > 2.0 {
        flags.insert("Z_WARN".into());
        notes.push(format!(
            "sources disagree at z={z_max:.2} (2<z≤3): fused, but check"
        ));
    }
    let (evidence_value, _, _) = weighted(&observations)?;
    let outside = bounds.is_some_and(|b| evidence_value < b[0] || evidence_value > b[1]);
    let prior_used = prior.is_some() && use_prior && !outside;
    if let Some([lo, hi]) = bounds.filter(|_| outside) {
        flags.insert("VERIFY_OUT_OF_BOUNDS".into());
        notes.push(format!("measured {evidence_value:.1} is outside the chart P1–P99 [{lo:.1}, {hi:.1}]: confirm once; chart prior left out of the fusion"));
    }
    if prior_used {
        observations.push(prior.as_ref().unwrap().clone());
    }
    let (value, sigma, weights) = weighted(&observations)?;
    let mut totals: Vec<(&str, f64)> = Vec::new();
    for (obs, w) in observations.iter().zip(&weights) {
        if let Some((_, total)) = totals.iter_mut().find(|(p, _)| *p == obs.provenance()) {
            *total += w;
        } else {
            totals.push((obs.provenance(), *w));
        }
    }
    let mut primary = totals[0];
    for candidate in totals.into_iter().skip(1) {
        if candidate.1 > primary.1 {
            primary = candidate;
        }
    }
    let mut sources: Vec<Value> = observations
        .iter()
        .zip(&weights)
        .map(|(o, w)| o.source(u, *w, true))
        .collect();
    if !prior_used && let Some(p) = prior {
        sources.push(p.source(u, 0.0, false));
    }
    let mut out = entry(u, value, sigma, primary.0, sources, &flags, bounds);
    if multiple {
        out["z_max"] = json!(py::round(z_max, 2));
    }
    if !notes.is_empty() {
        out["notes"] = json!(notes.join("; "));
    }
    Ok(Some(out))
