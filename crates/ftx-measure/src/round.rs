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

/// Select garment checks from pattern piece roles, including closed cuffs and nappy/elastic-waist
/// requirements. This function is clock-free and does not depend on pattern or browser types.
/// Legacy flat measurement maps have unknown provenance and return `NOT_CHECKED`, not `READY`.
/// A failed readiness computation is likewise explicit, never silently promoted to evidence.
#[must_use]
pub fn round_readiness(pattern: &Value, measurements: &Value, as_of: &str) -> Value {
    let roles: Vec<&str> = pattern["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|p| p["role"].as_str())
        .collect();
    let has = |role| roles.contains(&role);
    let style = &pattern["meta"]["style"];
    let mut kinds = Vec::new();
    if has("bodice_front") || has("bodice_back") {
        kinds.push("bodice");
    }
    if has("sleeve") {
        kinds.push("set_in_sleeve");
    }
    if has("pant_front") || has("pant_back") {
        kinds.push(if has("cuff") {
            "full_trousers_gathered"
        } else {
            "trousers"
        });
    }
    if has("hat_crown") || has("hat_tip") {
        kinds.push("hat_fez");
    }
    let empty = |note: String| {
        json!({"schema":"fabric-round-readiness/1", "status":"NOT_CHECKED",
        "checks":[], "blocking":[], "remeasure_request":[], "advisories":[], "note":note})
    };
    if kinds.is_empty() {
        return empty("no garment roles in the pattern".into());
    }
    if measurements.get("records").is_none() {
        return empty("measurements have no records; provenance is unknown".into());
    }
    let options = ReadinessOptions {
        closed_cuff: true,
        elastic_waist: has("waistband"),
        cuff_level: style["cuff_level"].as_str().unwrap_or("calf"),
        nappies: (style["nappy"] == true).then_some(true),
        ..Default::default()
    };
    let (mut checks, mut blocking, mut requests, mut advisories, mut real) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for kind in kinds {
        let result = match readiness(measurements, kind, &options, as_of) {
            Ok(r) => r,
            Err(e) => return empty(format!("measurement readiness failed: {}", e.message)),
        };
        checks.push(
            json!({"garment":kind, "status":result["status"], "blocking":result["blocking"]}),
        );
        append_unique(&mut blocking, &result["blocking"]);
        append_unique(&mut requests, &result["remeasure_request"]);
        append_unique(&mut advisories, &result["advisories"]);
        append_unique(&mut real, &result["for_real_child"]);
    }
    let status = if checks.iter().any(|r| r["status"] == "NOT_READY") {
        "NOT_READY"
    } else if checks.iter().all(|r| r["status"] == "READY_REFERENCE") {
        "READY_REFERENCE"
    } else {
        "READY"
    };
    let mut out = json!({"schema":"fabric-round-readiness/1", "status":status, "checks":checks,
        "blocking":blocking, "remeasure_request":requests, "advisories":advisories});
    if status == "READY_REFERENCE" {
        out["advisories"] = json!([]);
        out["for_real_child"] = json!(real);
        out["note"] = json!(
            "Reference wearer built from public size charts; this is not measured evidence for a real wearer."
        );
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
