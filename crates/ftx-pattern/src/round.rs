//! Shared round policy and placement inference (spec 04 §3.5, §11.5–6).
//!
//! A verdict describes its inputs, not physical fit. The caller must keep the separate basis and
//! must not supply a simulation or export report from a different run.

use std::collections::BTreeMap;

use ftx_core::{Refusal, RefusalCode, py};
use serde_json::{Value, json};

use crate::{BodyMeasure, truthy};

/// First-match status precedence, shared by native and browser callers. Absent optional checks
/// do not block flat convergence; their absence must remain visible in the round's basis.
#[must_use]
pub fn round_status<'a>(
    flat: &'a str,
    sim: Option<&Value>,
    exports: Option<&Value>,
    readiness: &str,
    unscaled: bool,
) -> &'a str {
    if flat == "BLOCKED" {
        "BLOCKED"
    } else if unscaled {
        "UNSCALED"
    } else if flat != "CONVERGED" {
        flat
    } else if sim.is_some_and(|s| truthy(&s["gating"])) {
        "SIM_GATING_FLAGS"
    } else if exports.is_some_and(|e| e["pass"] != true) {
        "EXPORT_CHECKS_FAILED"
    } else if readiness == "NOT_READY" {
        "MEASUREMENTS_NOT_READY"
    } else {
        "CONVERGED"
    }
}

/// Compare only point identifiers and target bands, as the workspace fit-contract guard does.
/// Lists are sorted and deterministic; the workspace contract is never changed by this function.
#[must_use]
pub fn fit_contract_changes(workspace: &Value, draft: &Value) -> Value {
    let bands = |spec: &Value| -> BTreeMap<String, Value> {
        spec["points"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|p| Some((p["id"].as_str()?.to_string(), p["target_mm"].clone())))
            .collect()
    };
    let (old, new) = (bands(workspace), bands(draft));
    json!({
        "new": new.keys().filter(|k| !old.contains_key(*k)).collect::<Vec<_>>(),
        "missing": old.keys().filter(|k| !new.contains_key(*k)).collect::<Vec<_>>(),
        "changed": new.iter().filter(|(k, v)| old.get(*k).is_some_and(|o| o != *v))
            .map(|(k, _)| k).collect::<Vec<_>>(),
    })
}

/// Infer missing placements from piece roles in millimetres. Existing placements and `sim: false`
/// are preserved. Pixel-unit patterns are returned unchanged. All changes are made on a copy, so
/// a missing body measurement cannot leave a half-edited pattern.
pub fn infer_placements(
    pattern: &Value,
    body: &impl BodyMeasure,
) -> Result<(Value, Vec<String>), Refusal> {
    let mut out = pattern.clone();
    let mut notes = Vec::new();
    if pattern["meta"]["scale_status"] == "UNSCALED" {
        return Ok((out, notes));
    }
    let invalid = |s: String| Refusal::new(RefusalCode::InvalidDocument, s);
    let pieces = out["pieces"]
        .as_array_mut()
        .ok_or_else(|| invalid("pattern pieces must be an array".into()))?;
    for piece in pieces {
        if truthy(&piece["placement"]) || piece["sim"] == false {
            continue;
        }
        let id = piece["id"].as_str().unwrap_or("?").to_string();
        let role = piece["role"].as_str().unwrap_or("").to_string();
        if piece.get("garmentcode_source").is_some()
            && matches!(role.as_str(), "sleeve" | "pant_front" | "pant_back")
        {
            notes.push(format!(
                "{id}: imported physical limb panel needs explicit placement.side R or L and placement anchors; excluded from the fold-up"
            ));
            continue;
        }
        if !matches!(
            role.as_str(),
            "bodice_front"
                | "bodice_back"
                | "skirt_front"
                | "skirt_back"
                | "pant_front"
                | "pant_back"
                | "sleeve"
        ) {
            notes.push(format!(
                "{id}: no placement (role '{role}' is not simulated); excluded from the fold-up"
            ));
            continue;
        }
        let vertices = piece["vertices"]
            .as_object()
            .ok_or_else(|| invalid(format!("{id}: vertices must be an object")))?;
        let mut points = Vec::new();
        for v in vertices.values() {
            let p = v
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or_else(|| invalid(format!("{id}: invalid vertex")))?;
            let x = p[0].as_f64().filter(|x| x.is_finite());
            let y = p[1].as_f64().filter(|y| y.is_finite());
            points.push(
                x.zip(y)
                    .ok_or_else(|| invalid(format!("{id}: non-finite vertex")))?,
            );
        }
        let &(mut top_x, mut ymax) = points
            .first()
            .ok_or_else(|| invalid(format!("{id}: no vertices")))?;
        let (mut xmin, mut xmax) = (top_x, top_x);
        for &(x, y) in &points {
            xmin = xmin.min(x);
            xmax = xmax.max(x);
            if y > ymax {
                (top_x, ymax) = (x, y);
            }
        }
        let mut placement = match role.as_str() {
            "bodice_front" | "bodice_back" => {
                let snp = body
                    .landmark_h("torso", "snp")
                    .map_err(|e| invalid(e.to_string()))?;
                let waist = body
                    .landmark_h("torso", "waist")
                    .map_err(|e| invalid(e.to_string()))?;
                json!({"part":"torso", "theta_deg":if role == "bodice_front" {-4.8} else {180.0},
                    "x0":if role == "bodice_front" {xmax} else {xmin},
                    "y0_landmark":"waist", "y0":py::round(ymax - (snp + 10.0 - waist), 2), "clearance":20.0})
            }
            "skirt_front" | "skirt_back" => json!({"part":"torso",
                "theta_deg":if role == "skirt_front" {0.0} else {180.0},
                "x0":if role == "skirt_front" {xmax} else {xmin},
                "y0_landmark":"waist", "y0":py::round(ymax, 2), "clearance":25.0}),
            "pant_front" | "pant_back" => json!({"part":"leg",
                "theta_deg":if role == "pant_front" {0.0} else {180.0},
                "x0":py::round(0.5 * xmin + 0.5 * xmax, 2),
                "y0_landmark":"waist", "y0":py::round(ymax, 2), "clearance":25.0}),
            _ => {
                let arm = body
                    .value("upper_arm")
                    .map_err(|e| invalid(e.to_string()))?;
                let tau = 2.0 * std::f64::consts::PI;
                json!({"part":"arm", "theta_deg":-90.0, "x0":top_x,
                    "y0_landmark":"shoulder_tip", "y0":ymax,
                    "radius":py::round(((xmax - xmin) / tau).max(1.15 * arm / tau + 6.0), 2)})
            }
        };
        for name in ["x0", "y0", "radius"] {
            if placement.get(name).is_some()
                && placement[name].as_f64().is_none_or(|x| !x.is_finite())
            {
                return Err(invalid(format!("{id}: inferred {name} is not finite")));
            }
        }
        placement["inferred"] = json!(true);
        notes.push(format!(
            "{id}: placement inferred from role '{role}' ({}, theta {})",
            placement["part"].as_str().unwrap(),
            placement["theta_deg"]
        ));
        piece["placement"] = placement;
    }
    Ok((out, notes))
}
