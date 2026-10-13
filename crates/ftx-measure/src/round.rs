//! Role-driven measurement gate for a whole garment (spec 04 §11.6).

use serde_json::{Value, json};

use crate::{ReadinessOptions, readiness};

fn append_unique(out: &mut Vec<Value>, values: &Value) {
    for v in values.as_array().into_iter().flatten() {
        if !out.contains(v) {
            out.push(v.clone());
        }
    }
}

/// The measurement gates a pattern needs, from its piece roles, each with its own options (spec 04
/// §11.6, the reference's `readiness_checks`): a sleeve's closed cuff, trousers' elastic waist, a
/// gathered cuff's level and nappies.
#[must_use]
pub fn garment_checks(pattern: &Value) -> Vec<(&'static str, ReadinessOptions<'_>)> {
    let roles: Vec<&str> = pattern["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["role"].as_str())
        .collect();
    let has = |role| roles.contains(&role);
    // `style if isinstance(style, dict) else {}`; `style.get("cuff_level", "calf")` passes a present
    // non-string on (the gate then refuses it); `True if style.get("nappy") else None` is Python truthiness
    let style = pattern["meta"]["style"].as_object();
    let cuff_level = match style.and_then(|s| s.get("cuff_level")) {
        None => "calf",
        Some(Value::String(s)) => s.as_str(),
        Some(_) => "",
    };
    let nappy = style.and_then(|s| s.get("nappy")).is_some_and(|v| match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|x| x != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    });
    let elastic_waist = has("waistband");
    let mut out = Vec::new();
    if has("bodice_front") || has("bodice_back") {
        out.push(("bodice", ReadinessOptions::default()));
    }
    if has("sleeve") {
        out.push((
            "set_in_sleeve",
            ReadinessOptions {
                closed_cuff: true,
                ..Default::default()
            },
        ));
    }
    if has("pant_front") || has("pant_back") {
        out.push(if has("cuff") {
            (
                "full_trousers_gathered",
                ReadinessOptions {
                    elastic_waist,
                    cuff_level,
                    nappies: nappy.then_some(true),
                    ..Default::default()
                },
            )
        } else {
            (
                "trousers",
                ReadinessOptions {
                    elastic_waist,
                    ..Default::default()
                },
            )
        });
    }
    if has("hat_crown") || has("hat_tip") {
        out.push(("hat_fez", ReadinessOptions::default()));
    }
    out
}

/// Select garment checks from pattern piece roles ([`garment_checks`]). This function is clock-free
/// and does not depend on pattern or browser types. Legacy flat measurement maps have unknown
/// provenance and return `NOT_CHECKED`, not `READY`. A failed readiness computation is likewise
/// explicit, never silently promoted to evidence.
#[must_use]
pub fn round_readiness(pattern: &Value, measurements: &Value, as_of: &str) -> Value {
    let kinds = garment_checks(pattern);
    let empty = |note: String| {
        json!({"schema":"fabric-round-readiness/1", "status":"NOT_CHECKED",
        "checks":[], "blocking":[], "remeasure_request":[], "advisories":[], "note":note})
    };
    if kinds.is_empty() {
        return empty("no piece roles: cannot tell which measurements this garment needs".into());
    }
    if measurements.get("records").is_none() {
        return empty(
            "the measurements file has no records (plain name -> value): provenance unknown".into(),
        );
    }
    let (mut checks, mut blocking, mut requests, mut advisories, mut real) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let mut failed = None;
    for (kind, options) in kinds {
        let result = match readiness(measurements, kind, &options, as_of) {
            Ok(r) => r,
            Err(e) => {
                // what was gathered so far stays, as the reference returns it
                failed = Some(format!(
                    "{kind}: readiness could not be computed ({})",
                    e.message
                ));
                break;
            }
        };
        checks.push(
            json!({"garment":result["garment"], "status":result["status"], "blocking":result["blocking"]}),
        );
        append_unique(&mut blocking, &result["blocking"]);
        append_unique(&mut requests, &result["remeasure_request"]);
        // a reference body's advisories are about its priors, not a wearer
        if result["status"] != "READY_REFERENCE" {
            append_unique(&mut advisories, &result["advisories"]);
        }
        append_unique(&mut real, &result["for_real_child"]);
    }
    let status = if failed.is_some() {
        "NOT_CHECKED"
    } else if checks.iter().any(|r| r["status"] == "NOT_READY") {
        "NOT_READY"
    } else if checks.iter().all(|r| r["status"] == "READY_REFERENCE") {
        "READY_REFERENCE"
    } else {
        "READY"
    };
    let mut out = json!({"schema":"fabric-round-readiness/1", "status":status, "checks":checks,
        "blocking":blocking, "remeasure_request":requests, "advisories":advisories});
    if !real.is_empty() {
        out["for_real_child"] = json!(real);
    }
    if let Some(note) = failed {
        out["note"] = json!(note);
    }
    out
}

/// Human-readable gate including actionable requests; no printing in the library.
#[must_use]
pub fn format_round_readiness(report: &Value) -> String {
    let kinds = report["checks"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|r| r["garment"].as_str())
        .collect::<Vec<_>>()
        .join(", ");
    let mut text = match report["status"].as_str().unwrap_or("NOT_CHECKED") {
        "NOT_READY" => {
            format!("MEASUREMENTS: NOT_READY for {kinds}. Re-measure before trusting the fit:")
        }
        "READY_REFERENCE" => format!(
            "MEASUREMENTS: READY_REFERENCE for {kinds} (a reference wearer built from public size charts; a real wearer needs {} measurements taken)",
            report["for_real_child"].as_array().map_or(0, Vec::len)
        ),
        "READY" => format!("MEASUREMENTS: READY for {kinds}"),
        _ => format!(
            "MEASUREMENTS: not checked ({})",
            report["note"].as_str().unwrap_or("unknown provenance")
        ),
    };
    for (key, prefix) in [("remeasure_request", "  "), ("advisories", "  note: ")] {
        for line in report[key]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
        {
            text.push_str(&format!("\n{prefix}{line}"));
        }
    }
    text
}
