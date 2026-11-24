//! Local-only, portable workspace review: measured evidence, flat construction, physics and exports
//! stay separate. All user text is escaped; ledger paths cannot introduce external or parent links.

use std::fmt::Write;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_export::text::{fnum, xml_escape};
use serde_json::Value;

fn cell(value: &Value) -> String {
    match value {
        Value::Null => "—".into(),
        Value::String(s) => xml_escape(s),
        Value::Number(n) => n.as_f64().map_or_else(|| n.to_string(), |v| fnum(v, 1)),
        Value::Array(a) => a.iter().map(cell).collect::<Vec<_>>().join(" / "),
        other => xml_escape(&other.to_string()),
    }
}

fn list(out: &mut String, values: &Value) {
    for value in values.as_array().into_iter().flatten() {
        let _ = writeln!(out, "<p class=\"item\">{}</p>", cell(value));
    }
}

fn table(out: &mut String, title: &str, rows: &Value, columns: &[(&str, &str)]) {
    let Some(rows) = rows.as_array().filter(|r| !r.is_empty()) else {
        return;
    };
    let _ = write!(
        out,
        "<h3>{title}</h3><div class=\"scroll\"><table><thead><tr>"
    );
    for (_, label) in columns {
        let _ = write!(out, "<th>{label}</th>");
    }
    out.push_str("</tr></thead><tbody>");
    for row in rows {
        let class = match row["status"].as_str().unwrap_or("") {
            "PASS" | "EASY" | "CONVERGED" => "pass",
            "FAIL" | "ERROR" | "TIGHT" | "UNMEASURABLE" => "fail",
            _ => "",
        };
        let _ = write!(out, "<tr class=\"{class}\">");
        for (key, _) in columns {
            let _ = write!(out, "<td>{}</td>", cell(&row[*key]));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table></div>");
}

fn static_equilibrium(out: &mut String, eq: &Value) {
    if !eq.is_object() {
        return;
    }
    let status = eq["status"].as_str().unwrap_or("unknown");
    let _ = write!(
        out,
        "<h4>Static equilibrium comparison (frictionless)</h4><p>Status: {}",
        xml_escape(status)
    );
    if status == "available" {
        if let Some(res) = eq["result"].as_object() {
            if let Some(iters) = res.get("solve").and_then(|s| s["iterations"].as_u64()) {
                let _ = write!(out, ". Newton iterations: {iters}");
            }
            if let Some(max_mm) = res
                .get("max_displacement_m")
                .and_then(Value::as_f64)
                .map(|v| v * 1000.0)
                .filter(|v| v.is_finite())
            {
                let _ = write!(out, ". Max displacement: {} mm", fnum(max_mm, 3));
            }
            if let Some(rms_mm) = res
                .get("rms_displacement_m")
                .and_then(Value::as_f64)
                .map(|v| v * 1000.0)
                .filter(|v| v.is_finite())
            {
                let _ = write!(out, ". RMS displacement: {} mm", fnum(rms_mm, 3));
            }
        }
        out.push_str(".</p>");
        if let Some(no_claim) = eq["no_claim"].as_str() {
            let _ = write!(out, "<p class=\"muted\">{}</p>", xml_escape(no_claim));
        }
    } else if status == "refused" {
        if let Some(msg) = eq["refusal"]["message"].as_str() {
            let _ = write!(out, " — {}", xml_escape(msg));
        }
        out.push_str(". Dynamic trajectory and flags remain authoritative.</p>");
    } else {
        out.push_str(".</p>");
    }
}

fn imported_body(out: &mut String, body: &Value) {
    if body["shape_source"]["kind"] != "accepted_multi_frame_fit" {
        return;
    }
    out.push_str("<h3>Body used for draping — MODEL_FIT</h3>\
        <p>The imported shape is a model estimate. Compare it with the current measurement targets before \
        using the drape. Residual = model minus target; positive means the model is larger. \
        Targets retain their existing evidence basis, including chart priors. These comparisons do not \
        refit the body, change measurement readiness or certify garment fit.</p>");
    let _ = write!(
        out,
        "<p>Source image-fit status: {}. Used a new fit: {}. Reused image poses: {}. \
        Report SHA-256: <code>{}</code>.</p>",
        cell(&body["source_fit"]["status"]),
        cell(&body["shape_source"]["refitted"]),
        cell(&body["shape_source"]["image_poses_reused"]),
        cell(&body["shape_source"]["report_sha256"])
    );
    let mut rows = body["measurement_comparison"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for row in &mut rows {
        // Keep the stored residual's precision; ordinary construction tables round to 0.1 mm.
        for key in ["target_mm", "body_mm", "residual_mm"] {
            if row[key].is_number() {
                row[key] = Value::String(row[key].to_string());
            }
        }
    }
    if rows.is_empty() {
        out.push_str("<p>No current measurement targets were supplied for comparison.</p>");
    } else {
        table(
            out,
            "Imported body vs current targets — millimetres",
            &Value::Array(rows),
            &[
                ("name", "Measurement"),
                ("target_mm", "Current target"),
                ("body_mm", "MODEL_FIT body"),
                ("residual_mm", "Signed residual"),
            ],
        );
    }
    if body["tape_evaluation"]["status"] == "UNAVAILABLE" {
        let _ = write!(
            out,
            "<p>Some model tape predictions are unavailable: {}. \
            Missing predictions are not zero.</p>",
            cell(&body["tape_evaluation"]["reason"])
        );
    }
}

fn validation_body(out: &mut String, body: &Value) {
    if !body.is_object() {
        return;
    }
    out.push_str("<h3>Body used for flat validation — MODEL_FIT</h3>\
        <p>Finished dimensions and ease were checked in the selected body's source measurement pose. \
        Original measurement evidence and readiness remain separate. These model predictions do not \
        establish physical garment fit.</p>");
    let _ = write!(
        out,
        "<p>Shape source: {}. Measurement pose: {}. Model SHA-256: <code>{}</code>.</p>",
        cell(&body["identity"]["reference"]["shape_source"]["kind"]),
        cell(&body["identity"]["reference"]["measurement_pose"]),
        cell(&body["identity"]["reference"]["model_sha256"])
    );
    let mut rows = body["body_fit"]["residuals"]
        .as_array()
        .cloned()
        .unwrap_or_default();
    for row in &mut rows {
        for key in ["target_mm", "body_mm", "residual_mm"] {
            if row[key].is_number() {
                row[key] = Value::String(row[key].to_string());
            }
        }
    }
    table(
        out,
        "Selected body vs supplied targets — millimetres; residual = model minus target",
        &Value::Array(rows),
        &[
            ("name", "Measurement"),
            ("target_mm", "Supplied target"),
            ("body_mm", "MODEL_FIT body"),
            ("residual_mm", "Signed residual"),
        ],
    );
}

fn round_dir(value: &Value) -> Option<&str> {
    let dir = value.as_str()?;
    let (base, number) = dir.split_once('/')?;
    (base == "rounds" && !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        .then_some(dir)
}

fn local_path(value: &str) -> bool {
    !value.is_empty()
        && value.split('/').all(|part| {
            !part.is_empty()
                && part != "."
                && part != ".."
                && part
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
        })
}

fn regular_file(path: &Path) -> Result<bool, Refusal> {
    match std::fs::symlink_metadata(path) {
        Ok(m) => Ok(m.is_file() && !m.file_type().is_symlink()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(super::io(path, e)),
    }
}

fn link(out: &mut String, ws: &Path, path: &str, label: &str) -> Result<(), Refusal> {
    if !local_path(path) {
        return Ok(());
    }
    let parts: Vec<&str> = path.split('/').collect();
    let mut parent = ws.to_path_buf();
    for part in &parts[..parts.len() - 1] {
        parent.push(part);
        if !super::regular(&parent, true)? {
            return Ok(());
        }
    }
    if regular_file(&ws.join(path))? {
        let _ = write!(
            out,
            "<a href=\"{}\">{}</a> ",
            xml_escape(path),
            xml_escape(label)
        );
    }
    Ok(())
}

fn exports(out: &mut String, ws: &Path, directory: &str) -> Result<(), Refusal> {
    let root = ws.join(directory);
    if !super::regular(&root, true)? {
        return Ok(());
    }
    let mut pending = vec![directory.to_string()];
    let mut paths = Vec::new();
    let mut visited = 0usize;
    while let Some(relative) = pending.pop() {
        let path = ws.join(&relative);
        for item in std::fs::read_dir(&path).map_err(|e| super::io(&path, e))? {
            visited += 1;
            if visited > 1000 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "workspace report exceeds 1000 export entries",
                ));
            }
            let item = item.map_err(|e| super::io(&path, e))?;
            let Some(name) = item.file_name().to_str().map(str::to_string) else {
                continue;
            };
            let rel = format!("{relative}/{name}");
            if !local_path(&rel) {
                continue;
            }
            let ty = item.file_type().map_err(|e| super::io(&item.path(), e))?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                pending.push(rel);
            } else if ty.is_file() {
                paths.push(rel);
            }
            if pending.len() + paths.len() > 1000 {
                return Err(Refusal::new(
                    RefusalCode::BudgetExceeded,
                    "workspace report exceeds 1000 export files/directories",
                ));
            }
        }
    }
    paths.sort();
    out.push_str("<div class=\"downloads\">");
    for path in paths {
        link(
            out,
            ws,
            &path,
            path.strip_prefix(directory)
                .unwrap_or(&path)
                .trim_start_matches('/'),
        )?;
    }
    out.push_str("</div>");
    Ok(())
}

/// Rendering reads only the saved ledger and the existence of local deliverables, never today's
/// measurement file. Updating the workspace body cannot revise an older round's explanation.
pub(super) fn render(ws: &Path, entries: &[Value]) -> Result<String, Refusal> {
    let mut out = String::from(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; img-src 'self' file:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'">
<title>FrankenTextiles — construction review</title>
<style>
:root{color-scheme:light dark}body{font:16px/1.5 system-ui,sans-serif;margin:0;background:#f2f4f5;color:#202a31}main{max-width:1200px;margin:auto;padding:28px}h1,h2{line-height:1.2}h2{margin-top:0}h3{margin-bottom:8px}article{padding:24px;background:white;border:1px solid #d5dce0;border-radius:12px;margin:24px 0}.warning{padding:16px;border-left:5px solid #9b650b;background:#fff4d8}.basis{display:grid;grid-template-columns:repeat(auto-fit,minmax(230px,1fr));gap:10px}.basis p{padding:12px;background:#edf1f5;margin:0}.basis strong{display:block}.scroll{overflow-x:auto}table{border-collapse:collapse;width:100%;font-size:14px}th,td{text-align:left;padding:8px;border-bottom:1px solid #d5dce0;vertical-align:top}th{white-space:nowrap}.fail{background:#fff0ee}.pass td:last-child{color:#146745}a{color:#174a91}p{overflow-wrap:anywhere}.item{margin:6px 0}.downloads{display:flex;flex-wrap:wrap;gap:12px;margin:12px 0}.preview{display:block;max-width:100%;height:auto;border:1px solid #c5cbd0;margin:16px 0}pre{white-space:pre-wrap;overflow-wrap:anywhere;font:13px/1.5 ui-monospace,monospace}details{margin:16px 0}.muted{color:#526471}.dark{display:none}@media(prefers-color-scheme:dark){body{background:#14191f;color:#e8eef3}article{background:#1c242d;border-color:#3b4652}.basis p{background:#293442}.warning{background:#3b311a}.fail{background:#462926}.pass td:last-child{color:#9ddebc}a{color:#a4c8ff}.muted{color:#bdcad5}.light{display:none}.dark{display:block}th,td{border-color:#3b4652}}
</style></head><body><main><h1>Construction review</h1>
<p class="warning"><strong>These are predictions and file checks, not proof of fit.</strong> Flat construction, drape flags, manufacturing checks and measurement readiness are reported separately. A chart-reference result fits the chart, not a real wearer. Confirm fit with a muslin or dressed photo.</p>
"#,
    );
    if entries.is_empty() {
        out.push_str("<p>No completed rounds have been recorded.</p>");
    }
    for entry in entries {
        let _ = write!(
            out,
            "<article><h2>Round {} — {}</h2><p class=\"muted\">{}</p><p>{}</p>",
            cell(&entry["round"]),
            cell(&entry["status"]),
            cell(&entry["at"]),
            cell(&entry["note"])
        );
        out.push_str("<div class=\"basis\">");
        for (key, name) in [
            ("flat", "Flat construction"),
            ("sim", "Drape prediction"),
            ("exports", "Manufacturing checks"),
            ("measurements", "Measurement evidence"),
        ] {
            let _ = write!(
                out,
                "<p><strong>{name}</strong>{}</p>",
                cell(&entry["basis"][key])
            );
        }
        out.push_str("</div>");
        list(&mut out, &entry["auto_notes"]);
        if let Some(dir) = round_dir(&entry["dir"]) {
            // Check the parent itself: a ledger's valid-looking number may name an external symlink.
            if super::regular(&ws.join(dir), true)? {
                for (name, class, alt) in [
                    ("preview.svg", "light", "Construction preview"),
                    ("preview_dark.svg", "dark", "Construction preview (dark)"),
                ] {
                    let path = format!("{dir}/{name}");
                    if regular_file(&ws.join(&path))? {
                        let _ = write!(
                            out,
                            "<img class=\"preview {class}\" src=\"{path}\" alt=\"{alt}\">"
                        );
                    }
                }
                out.push_str("<div class=\"downloads\">");
                for (name, label) in [
                    ("pattern.json", "Pattern"),
                    ("fit_spec.json", "Fit contract"),
                    ("measurements.source.json", "Original evidence"),
                    ("measurements.json", "Fused measurements"),
                    ("readiness.json", "Readiness report"),
                    ("validation.json", "Validation report"),
                    ("validation_body.json", "Flat validation body"),
                    ("body_fit.json", "Original accepted body fit"),
                    ("round.json", "Round record"),
                ] {
                    link(&mut out, ws, &format!("{dir}/{name}"), label)?;
                }
                out.push_str("</div>");
            }
        }
        let ready = &entry["readiness"];
        let _ = write!(
            out,
            "<h3>Measurement readiness: {}</h3>",
            cell(&ready["status"])
        );
        if let Some(note) = ready["note"].as_str() {
            let _ = write!(out, "<p>{}</p>", xml_escape(note));
        }
        if ready["status"] == "READY_REFERENCE" {
            out.push_str("<p class=\"warning\">Reference chart only. The following measurements are still needed for the real wearer.</p>");
        }
        for key in ["remeasure_request", "for_real_child", "advisories"] {
            list(&mut out, &ready[key]);
        }
        validation_body(&mut out, &entry["validation_body"]);
        let val = &entry["validation"];
        list(&mut out, &val["structure_errors"]);
        list(&mut out, &val["completeness"]);
        table(
            &mut out,
            "Finished measurements — millimetres; authority: flat pattern",
            &val["points"],
            &[
                ("label", "Point"),
                ("body_mm", "Body"),
                ("sigma_mm", "Body σ"),
                ("finished_mm", "Finished"),
                ("ease_mm", "Ease"),
                ("target_mm", "Target min / nom / max"),
                ("status", "Status"),
            ],
        );
        table(
            &mut out,
            "Seam walk — millimetres",
            &val["seams"],
            &[
                ("seam", "Seam"),
                ("len_a_mm", "Side A"),
                ("len_b_mm", "Side B"),
                ("target_ratio", "Target ratio"),
                ("residual_mm", "Residual"),
                ("message", "Diagnostic"),
                ("status", "Status"),
            ],
        );
        table(
            &mut out,
            "Truing — degrees",
            &val["truing"],
            &[
                ("seam", "Seam"),
                ("end", "End"),
                ("line", "Line"),
                ("angle_sum_deg", "Angle sum"),
                ("target_deg", "Target"),
                ("status", "Status"),
            ],
        );
        list(&mut out, &val["worst"]);
        let sim = &entry["sim"];
        if sim.is_object() {
            imported_body(&mut out, &sim["body"]);
            out.push_str(&crate::two_start::review_html(sim));
            let _ = write!(
                out,
                "<h3>Drape prediction</h3><p>Settled: {}. Self-contact enabled: {}.",
                cell(&sim["settled"]),
                cell(&sim["self_contact"]["enabled"])
            );
            let legs = sim["body"]["legs"].as_str().filter(|v| *v != "standing");
            let leg_frac = sim["body"]
                .get("leg_fraction")
                .and_then(Value::as_f64)
                .filter(|v| v.is_finite() && *v != 1.0);
            if let Some(frac) = leg_frac {
                let _ = write!(
                    out,
                    " Leg posture: {} (fraction {frac}).",
                    xml_escape(legs.unwrap_or("standing"))
                );
            } else if let Some(legs) = legs {
                let _ = write!(out, " Leg posture: {}.", xml_escape(legs));
            }
            if let Some(skinning) = sim["body"]["skinning"].as_str() {
                let _ = write!(out, " Skinning: {}.", xml_escape(skinning));
            }
            if let Some(hands) = sim["hand_collider"].as_str() {
                let _ = write!(out, " Hand collider: {}.", xml_escape(hands));
            }
            if let Some(thumbs) = sim["body"]["thumb_pose"].as_object() {
                let fmt_vec = |key: &str| {
                    thumbs
                        .get(key)
                        .and_then(Value::as_array)
                        .filter(|a| a.len() == 3)
                        .and_then(|a| {
                            a.iter()
                                .map(|v| v.as_f64().filter(|x| x.is_finite()).map(|x| fnum(x, 4)))
                                .collect::<Option<Vec<_>>>()
                        })
                        .map(|vs| format!("[{}]", vs.join(", ")))
                };
                if let (Some(l), Some(r)) = (
                    fmt_vec("left_rotation_vector_rad"),
                    fmt_vec("right_rotation_vector_rad"),
                ) {
                    let _ = write!(out, " Thumb pose (rad): L {l}, R {r}");
                    if let Some(pairs) = sim["body"]["authored_hand_surface_pairs_tested"].as_u64()
                    {
                        let _ = write!(out, " ({pairs} hand-surface pairs audited)");
                    }
                    out.push('.');
                }
            }
            if let Some(scheme) = sim["membrane_history"]["integration"]
                .as_str()
                .filter(|s| *s != "backward_euler")
            {
                let _ = write!(out, " Relaxation integration: {}.", xml_escape(scheme));
            }
            if let Some(damage_model) = sim["membrane_plasticity_history"]["damage_model"].as_str()
            {
                let _ = write!(out, " Membrane damage: {}", xml_escape(damage_model));
                if let Some(dmg_j) = sim["membrane_plasticity_history"]["damage_dissipation_j"]
                    .as_f64()
                    .filter(|v| v.is_finite())
                {
                    let _ = write!(out, " ({dmg_j:.3e} J)");
                }
                out.push('.');
            }
            if let Some(kind) = sim["numerical"]["newton_strategy"]["kind"].as_str() {
                let _ = write!(out, " Newton solver: {}.", xml_escape(kind));
            }
            if let Some(ig) = sim["numerical"]["integrator"].as_object()
                && let Some(kind) = ig.get("kind").and_then(Value::as_str)
            {
                let _ = write!(out, " Integrator: {}", xml_escape(kind));
                if let Some(rho) = ig.get("rho_inf") {
                    let rho_text = rho
                        .as_f64()
                        .map_or_else(|| cell(rho), |v| xml_escape(&v.to_string()));
                    let _ = write!(out, " (ρ∞ = {rho_text})");
                }
                out.push('.');
            }
            if let Some(bm) = sim["numerical"]["bending_model"].as_str() {
                let _ = write!(out, " Bending model: {}.", xml_escape(bm));
            }
            if let Some(cm) = sim["numerical"]["compression_model"].as_str() {
                let _ = write!(out, " Compression model: {}.", xml_escape(cm));
            }
            if let Some(damp) = sim["numerical"]["damping"].as_object() {
                let mut parts = Vec::new();
                if let Some(drag) = damp.get("air_drag").and_then(Value::as_object) {
                    if let Some(wind) = drag.get("air_velocity_m_s").and_then(Value::as_array)
                        && wind.len() == 3
                        && let (Some(wx), Some(wy), Some(wz)) =
                            (wind[0].as_f64(), wind[1].as_f64(), wind[2].as_f64())
                        && (wx != 0.0 || wy != 0.0 || wz != 0.0)
                    {
                        parts.push(format!("air drag (wind [{wx}, {wy}, {wz}] m/s)"));
                    } else {
                        parts.push("air drag".to_string());
                    }
                }
                if let Some(rm) = damp.get("rayleigh_membrane_s").and_then(Value::as_f64)
                    && rm > 0.0
                {
                    parts.push(format!("Rayleigh membrane {rm:.3e} s"));
                }
                if let Some(rb) = damp.get("rayleigh_bending_s").and_then(Value::as_f64)
                    && rb > 0.0
                {
                    parts.push(format!("Rayleigh bending {rb:.3e} s"));
                }
                if !parts.is_empty() {
                    let _ = write!(out, " Damping: {}.", xml_escape(&parts.join(", ")));
                }
            }
            if sim["numerical"]["adaptive_barrier"].is_object() {
                out.push_str(" Adaptive IPC barrier: enabled.");
            }
            if let Some(ss) = sim["numerical"]["stick_slip"].as_object()
                && let (Some(mu_s), Some(mu_k)) = (
                    ss.get("mu_s").and_then(Value::as_f64),
                    ss.get("mu_k").and_then(Value::as_f64),
                )
            {
                let _ = write!(out, " Stick-slip friction: μs = {mu_s}, μk = {mu_k}.");
            }
            if let Some(ss) = sim["numerical"]["self_stick_slip"].as_object()
                && let (Some(mu_s), Some(mu_k)) = (
                    ss.get("mu_s").and_then(Value::as_f64),
                    ss.get("mu_k").and_then(Value::as_f64),
                )
            {
                let _ = write!(out, " Self stick-slip friction: μs = {mu_s}, μk = {mu_k}.");
            }
            if let Some(mesh) = sim["numerical"]["mesh"].as_object() {
                let mut parts = Vec::new();
                if let Some(min_s) = mesh
                    .get("options")
                    .and_then(|o| o.get("min_spacing_mm"))
                    .and_then(Value::as_f64)
                {
                    parts.push(format!("min spacing {min_s} mm"));
                }
                if let Some(cvt) = mesh
                    .get("options")
                    .and_then(|o| o.get("cvt_iterations"))
                    .and_then(Value::as_u64)
                    && cvt > 0
                {
                    parts.push(format!("{cvt} CVT passes"));
                }
                if let Some(min_ang) = mesh
                    .get("quality")
                    .and_then(|q| q.get("min_angle_deg"))
                    .and_then(Value::as_f64)
                {
                    parts.push(format!("min angle {min_ang:.1}°"));
                }
                if !parts.is_empty() {
                    let _ = write!(out, " Mesh: {}.", xml_escape(&parts.join(", ")));
                }
            }
            if let Some(constr) = sim["numerical"]["construction"].as_object()
                && let Some(opt) = constr.get("options").and_then(Value::as_object)
            {
                let mut parts = Vec::new();
                let hw = opt
                    .get("hem_width_mm")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let hm = opt
                    .get("hem_mass_multiplier")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                let hb = opt
                    .get("hem_bending_multiplier")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                if hw > 0.0 && (hm > 1.0 || hb > 1.0) {
                    parts.push(format!("hem {hw} mm ({hm}× mass, {hb}× bend)"));
                }
                let sw = opt
                    .get("seam_width_mm")
                    .and_then(Value::as_f64)
                    .unwrap_or(0.0);
                let sm = opt
                    .get("seam_mass_multiplier")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                let sb = opt
                    .get("seam_bending_multiplier")
                    .and_then(Value::as_f64)
                    .unwrap_or(1.0);
                if sm > 1.0 || sb > 1.0 {
                    parts.push(format!("seam {sw} mm ({sm}× mass, {sb}× bend)"));
                }
                if !parts.is_empty() {
                    let _ = write!(
                        out,
                        " Construction (uncalibrated): {}.",
                        xml_escape(&parts.join(", "))
                    );
                }
            }
            if let Some(rep) = sim["numerical"]["start_self_repulsion"].as_object()
                && let (Some(sep_m), Some(stiff), Some(period)) = (
                    rep.get("min_separation_m").and_then(Value::as_f64),
                    rep.get("stiffness").and_then(Value::as_f64),
                    rep.get("period").and_then(Value::as_u64),
                )
            {
                let _ = write!(
                    out,
                    " Start self-repulsion: {:.1} mm (stiffness {stiff}, period {period}",
                    sep_m * 1e3
                );
                if let Some(max_checks) = rep.get("max_candidate_checks").and_then(Value::as_u64) {
                    let _ = write!(out, ", max checks {max_checks}");
                }
                out.push_str(").");
            }
            if let Some(creases) = sim["numerical"]["authored_crease_hinges"]
                .as_u64()
                .filter(|&n| n > 0)
            {
                let _ = write!(out, " Authored creases: {creases} hinge(s).");
            }
            if let Some(settle) = sim["numerical"]["settling"].as_object()
                && let (Some(steps), Some(max_steps)) = (
                    settle.get("steps").and_then(Value::as_u64),
                    settle.get("max_steps").and_then(Value::as_u64),
                )
            {
                let _ = write!(out, " Extra settling: {steps}/{max_steps} steps");
                if let Some(reason) = settle.get("stop_reason").and_then(Value::as_str) {
                    let _ = write!(out, " ({})", xml_escape(reason));
                }
                out.push('.');
            }
            if let Some(sub) = sim["numerical"]["time_subdivision"].as_object()
                && let (Some(subdivided), Some(nominal), Some(accepted), Some(trials)) = (
                    sub.get("subdivided_nominal_intervals")
                        .and_then(Value::as_u64),
                    sub.get("nominal_intervals").and_then(Value::as_u64),
                    sub.get("accepted_substeps").and_then(Value::as_u64),
                    sub.get("trials").and_then(Value::as_u64),
                )
            {
                let _ = write!(
                    out,
                    " Time subdivision: {subdivided}/{nominal} intervals subdivided ({accepted} substeps, {trials} trials)."
                );
            }
            out.push_str("</p>");
            static_equilibrium(&mut out, &sim["static_equilibrium"]);
            out.push_str("<strong>Gating flags</strong>");
            list(&mut out, &sim["gating"]);
            out.push_str("<strong>Advisory flags</strong>");
            list(&mut out, &sim["advisory"]);
            if let Some(dir) = round_dir(&entry["dir"]) {
                if sim.get("refusal").is_some() {
                    link(
                        &mut out,
                        ws,
                        &format!("{dir}/sim_failure.json"),
                        "Simulation refusal and repairs",
                    )?;
                    for (view, label) in super::stills::VIEWS {
                        link(
                            &mut out,
                            ws,
                            &format!("{dir}/sim/refused/still_{view}.png"),
                            &format!("Refused run · {label}"),
                        )?;
                    }
                    // A run refused before its first step has no state of its own; it leaves the
                    // files of `ftx place` instead: the pre-sewn start, the pieces as wrapped and
                    // each piece's placement numbers.
                    link(
                        &mut out,
                        ws,
                        &format!("{dir}/sim/refused/placement.json"),
                        "Refused start · placements",
                    )?;
                    for (stage, name) in [("start", "Refused start"), ("wrap", "As wrapped")] {
                        for (view, label) in super::stills::VIEWS {
                            // The wrap stage's second colouring is how far each seam vertex has to
                            // travel to its partner, not strain.
                            let label = if stage == "wrap" {
                                label.replace("strain", "seam travel")
                            } else {
                                label.to_string()
                            };
                            link(
                                &mut out,
                                ws,
                                &format!("{dir}/sim/refused/{stage}/still_{view}.png"),
                                &format!("{name} · {label}"),
                            )?;
                        }
                    }
                }
                for (name, label) in [
                    ("fold_viewer.html", "Open drape viewer"),
                    ("sim_report.json", "Full drape report"),
                    ("body_fit.json", "Original accepted body fit"),
                    ("fold.pc2", "Cloth animation"),
                    ("garment.obj", "Garment mesh"),
                    ("body.obj", "Body mesh"),
                ] {
                    // Failed rounds never link any apparent success artifacts left by a failed writer.
                    if sim.get("refusal").is_none() {
                        link(&mut out, ws, &format!("{dir}/sim/{name}"), label)?;
                    }
                }
                if sim.get("refusal").is_none()
                    && entry["rendering"]["status"] == "PASS"
                    && super::regular(&ws.join(dir), true)?
                    && super::regular(&ws.join(format!("{dir}/sim")), true)?
                {
                    let mut has_standard = false;
                    let mut has_legible = false;
                    for (view, _) in super::stills::VIEWS {
                        let std_path = format!("{dir}/sim/still_{view}.png");
                        if entry["images"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v == &std_path))
                            && regular_file(&ws.join(&std_path))?
                        {
                            has_standard = true;
                        }
                        let leg_path = format!("{dir}/sim/still_{view}_legible.png");
                        if entry["images"]
                            .as_array()
                            .is_some_and(|a| a.iter().any(|v| v == &leg_path))
                            && regular_file(&ws.join(&leg_path))?
                        {
                            has_legible = true;
                        }
                    }
                    if has_standard || !has_legible {
                        out.push_str("<h3>Drape stills — construction and strain</h3>");
                        for (view, label) in super::stills::VIEWS {
                            let path = format!("{dir}/sim/still_{view}.png");
                            if entry["images"]
                                .as_array()
                                .is_some_and(|a| a.iter().any(|v| v == &path))
                                && regular_file(&ws.join(&path))?
                            {
                                let _ = write!(
                                    out,
                                    "<figure><figcaption>{label}</figcaption><img class=\"preview\" loading=\"lazy\" src=\"{path}\" alt=\"{label} drape prediction\"></figure>"
                                );
                            }
                        }
                    }
                    if has_legible {
                        out.push_str("<h3>Drape stills — legible shading (fold and edge aid; describes no fabric)</h3>");
                        for (view, label) in super::stills::VIEWS {
                            let path = format!("{dir}/sim/still_{view}_legible.png");
                            if entry["images"]
                                .as_array()
                                .is_some_and(|a| a.iter().any(|v| v == &path))
                                && regular_file(&ws.join(&path))?
                            {
                                let _ = write!(
                                    out,
                                    "<figure><figcaption>{label} (legible)</figcaption><img class=\"preview\" loading=\"lazy\" src=\"{path}\" alt=\"{label} legible drape prediction\"></figure>"
                                );
                            }
                        }
                    }
                }
            }
            let _ = write!(
                out,
                "<details><summary>Full physics, contact and body-fit details</summary><pre>{}</pre></details>",
                xml_escape(&serde_json::to_string_pretty(sim).map_err(|e| super::io(ws, e))?)
            );
        }
        if entry["rendering"]["status"] == "FAILED" {
            let _ = write!(
                out,
                "<p class=\"warning\"><strong>Stills failed; no rendered images published.</strong> {}. The physics and construction verdicts above are unchanged.</p>",
                cell(&entry["rendering"]["refusal"]["message"])
            );
        }
        out.push_str("<h3>Manufacturing files</h3>");
        let _ = write!(out, "<p>{}</p>", cell(&entry["basis"]["exports"]));
        list(&mut out, &entry["exports"]["failures"]);
        if !entry["exports"].is_null()
            && let Some(dir) = round_dir(&entry["dir"])
        {
            exports(&mut out, ws, &format!("{dir}/exports"))?;
        }
        out.push_str("</article>");
    }
    out.push_str("</main></body></html>\n");
    Ok(out)
}

pub(super) fn publish(ws: &Path) -> Result<(), Refusal> {
    let entries = super::ledger(ws)?;
    let html = render(ws, &entries)?;
    crate::workspace::replace(&ws.join("report.html"), html.as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn escaped_text_and_untrusted_paths_cannot_become_active_content() {
        let attack = "<script>alert('x')</script>";
        let entry = json!({"round":0,"dir":"rounds/../../outside","status":attack,"note":attack,
            "basis":{"flat":"NOT_CONVERGED","sim":"not run","exports":"not run","measurements":"NOT_READY"},
            "readiness":{"status":"NOT_READY","remeasure_request":[attack]},
            "validation":{"points":[{"label":attack,"body_mm":100.0,"finished_mm":110.0,"status":"TIGHT"}]},
            "viewer":"javascript:alert(1)","images":["https://bad.test/track"],"exports":null,
            "sim":{"body":{"shape_source":{"kind":"accepted_multi_frame_fit","report_sha256":attack},
                "source_fit":{"status":attack},"measurement_comparison":[{"name":attack}],
                "tape_evaluation":{"status":"UNAVAILABLE","reason":attack}}}});
        let html = render(Path::new("."), &[entry]).unwrap();
        assert!(!html.contains("<script>"));
        assert!(!html.contains("javascript:"));
        assert!(!html.contains("bad.test"));
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("not proof of fit"));
        assert!(html.contains("Content-Security-Policy"));
        assert!(round_dir(&json!("rounds/001")).is_some());
        for value in [
            "/rounds/001",
            "rounds/../001",
            "https://x/001",
            "rounds/001/extra",
        ] {
            assert!(round_dir(&json!(value)).is_none());
        }
        for path in [
            "../outside",
            "//server/share",
            "rounds/001/x?query",
            "rounds/001/x#hash",
        ] {
            assert!(!local_path(path));
        }
    }

    #[test]
    fn imported_body_residuals_keep_precision_missing_values_and_separate_readiness() {
        // Rendering-only saved ledger: numerical recovery is exercised by the real-CLI model test.
        let entry = json!({"round":0,"basis":{"measurements":"NOT_READY"},
            "readiness":{"status":"NOT_READY","remeasure_request":["independent tape needed"]},
            "sim":{"body":{"shape_source":{"kind":"accepted_multi_frame_fit",
                "report_sha256":"a".repeat(64),"refitted":false,"image_poses_reused":false},
                "source_fit":{"status":"ACCEPTED"},"measurement_comparison":[
                    {"name":"stature","target_mm":920,"body_mm":920.125,"residual_mm":0.125},
                    {"name":"chest","target_mm":540,"body_mm":515.125,"residual_mm":-24.875},
                    {"name":"waist","target_mm":530,"body_mm":null,"residual_mm":null}],
                "tape_evaluation":{"status":"UNAVAILABLE","reason":"missing synthetic anatomy"}}}});
        let html = render(Path::new("."), &[entry]).unwrap();
        for expected in [
            "Body used for draping — MODEL_FIT",
            "0.125",
            "-24.875",
            "<td>waist</td><td>530</td><td>—</td><td>—</td>",
            "Missing predictions are not zero",
            "Measurement readiness: NOT_READY",
            "independent tape needed",
            "including chart priors",
            "do not refit the body",
        ] {
            assert!(html.contains(expected), "missing {expected}");
        }
        assert!(!html.contains("Measurement readiness: READY"));
        let mut text = String::new();
        imported_body(&mut text, &json!({"readings_mm":{"stature":920}}));
        assert!(
            text.is_empty(),
            "ordinary tape body gained an image-fit basis"
        );
    }

    #[test]
    fn a_round_refused_at_its_start_links_the_placement_views_it_left() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let ws = std::env::temp_dir().join(format!(
            "ftx-round-start-views-{}-{stamp}",
            std::process::id()
        ));
        let refused = ws.join("rounds/003/sim/refused");
        std::fs::create_dir_all(refused.join("start")).unwrap();
        std::fs::create_dir_all(refused.join("wrap")).unwrap();
        std::fs::write(refused.join("placement.json"), "{}").unwrap();
        std::fs::write(refused.join("start/still_front.png"), "png").unwrap();
        std::fs::write(refused.join("start/still_strain_front.png"), "png").unwrap();
        std::fs::write(refused.join("wrap/still_strain_front.png"), "png").unwrap();
        let entry = json!({"round":3,"dir":"rounds/003",
            "sim":{"refusal":{"code":"intersecting_start"},"gating":["refused"],"advisory":[]}});
        let html = render(&ws, &[entry]).unwrap();
        assert!(html.contains(
            "href=\"rounds/003/sim/refused/placement.json\">Refused start · placements</a>"
        ));
        assert!(html.contains(
            "href=\"rounds/003/sim/refused/start/still_front.png\">Refused start · Front</a>"
        ));
        // The same file name is edge strain in the start and seam travel in the wrap.
        assert!(html.contains(
            "href=\"rounds/003/sim/refused/start/still_strain_front.png\">Refused start · Front strain</a>"
        ));
        assert!(html.contains(
            "href=\"rounds/003/sim/refused/wrap/still_strain_front.png\">As wrapped · Front seam travel</a>"
        ));
        // Only what was written is linked.
        assert!(!html.contains("refused/wrap/still_front.png"));
        assert!(!html.contains("refused/start/still_back.png"));
    }

    #[test]
    fn drape_summary_shows_opt_in_skinning_and_solver_controls_without_changing_defaults() {
        let default_entry = json!({
            "round": 0,
            "sim": {"settled": true, "self_contact": {"enabled": true}, "gating": [], "advisory": []}
        });
        let default_html = render(Path::new("."), &[default_entry]).unwrap();
        assert!(default_html.contains(
            "<h3>Drape prediction</h3><p>Settled: true. Self-contact enabled: true.</p>"
        ));
        assert!(!default_html.contains("Leg posture:"));
        assert!(!default_html.contains("Skinning:"));
        assert!(!default_html.contains("Hand collider:"));
        assert!(!default_html.contains("Thumb pose (rad):"));
        assert!(!default_html.contains("Relaxation integration:"));
        assert!(!default_html.contains("Membrane damage:"));
        assert!(!default_html.contains("Newton solver:"));
        assert!(!default_html.contains("Integrator:"));

        let opt_in_entry = json!({
            "round": 1,
            "sim": {
                "settled": true,
                "self_contact": {"enabled": true},
                "body": {"legs": "seated", "leg_fraction": 0.5, "skinning": "dqs"},
                "hand_collider": "palm",
                "membrane_history": {"integration": "exact_exponential"},
                "membrane_plasticity_history": {
                    "damage_model": "isotropic_accumulated_damage/1",
                    "damage_dissipation_j": 0.00125
                },
                "numerical": {
                    "newton_strategy": {"kind": "adaptive_ilu"},
                    "integrator": {"kind": "generalized_alpha", "rho_inf": 0.75}
                },
                "gating": [],
                "advisory": []
            }
        });
        let opt_in_html = render(Path::new("."), &[opt_in_entry]).unwrap();
        assert!(opt_in_html.contains(
            "<h3>Drape prediction</h3><p>Settled: true. Self-contact enabled: true. Leg posture: seated (fraction 0.5). Skinning: dqs. Hand collider: palm. Relaxation integration: exact_exponential. Membrane damage: isotropic_accumulated_damage/1 (1.250e-3 J). Newton solver: adaptive_ilu. Integrator: generalized_alpha (ρ∞ = 0.75).</p>"
        ));
        assert!(!default_html.contains("Extra settling:"));
        assert!(!default_html.contains("Time subdivision:"));
        assert!(!default_html.contains("Static equilibrium comparison"));
        assert!(!default_html.contains("Mesh:"));
        assert!(!default_html.contains("Construction (uncalibrated):"));
        assert!(!default_html.contains("Start self-repulsion:"));
        assert!(!default_html.contains("Authored creases:"));

        let extended = json!({
            "round": 2,
            "sim": {
                "settled": true,
                "self_contact": {"enabled": true},
                "body": {
                    "thumb_pose": {
                        "left_rotation_vector_rad": [0.0, 0.0, 0.1745],
                        "right_rotation_vector_rad": [0.0, 0.0, -0.1745]
                    },
                    "authored_hand_surface_pairs_tested": 4200
                },
                "hand_collider": "knuckle",
                "numerical": {
                    "damping": {
                        "air_drag": {
                            "rho_air_kg_m3": 1.225,
                            "c_normal": 1.2,
                            "c_tangent": 0.05,
                            "eps_v_m_s": 0.001,
                            "air_velocity_m_s": [2.5, -1.0, 0.5]
                        }
                    },
                    "mesh": {
                        "options": {"min_spacing_mm": 6.0, "cvt_iterations": 4},
                        "quality": {"min_angle_deg": 32.5}
                    },
                    "construction": {
                        "options": {
                            "hem_width_mm": 20.0,
                            "hem_mass_multiplier": 2.5,
                            "hem_bending_multiplier": 5.0,
                            "seam_width_mm": 10.0,
                            "seam_mass_multiplier": 2.0,
                            "seam_bending_multiplier": 3.0
                        }
                    },
                    "start_self_repulsion": {
                        "min_separation_m": 0.004,
                        "stiffness": 0.5,
                        "period": 5,
                        "max_candidate_checks": 500000
                    },
                    "authored_crease_hinges": 14,
                    "settling": {"steps": 4, "max_steps": 12, "stop_reason": "quiet_window"},
                    "time_subdivision": {
                        "subdivided_nominal_intervals": 2,
                        "nominal_intervals": 48,
                        "accepted_substeps": 52,
                        "trials": 56
                    }
                },
                "static_equilibrium": {
                    "status": "available",
                    "result": {
                        "solve": {"iterations": 9},
                        "max_displacement_m": 0.00425,
                        "rms_displacement_m": 0.000001
                    },
                    "no_claim": "Frictionless comparison only; <not> fit."
                },
                "gating": [],
                "advisory": []
            }
        });
        let ext_html = render(Path::new("."), &[extended]).unwrap();
        assert!(ext_html.contains("Damping: air drag (wind [2.5, -1, 0.5] m/s)."));
        assert!(ext_html.contains("Hand collider: knuckle."));
        assert!(ext_html.contains(
            "Thumb pose (rad): L [0, 0, 0.1745], R [0, 0, -0.1745] (4200 hand-surface pairs audited)."
        ));
        assert!(ext_html.contains("Mesh: min spacing 6 mm, 4 CVT passes, min angle 32.5°."));
        assert!(ext_html.contains(
            "Construction (uncalibrated): hem 20 mm (2.5× mass, 5× bend), seam 10 mm (2× mass, 3× bend)."
        ));
        assert!(ext_html.contains(
            "Start self-repulsion: 4.0 mm (stiffness 0.5, period 5, max checks 500000)."
        ));
        assert!(ext_html.contains("Authored creases: 14 hinge(s)."));
        assert!(ext_html.contains("Extra settling: 4/12 steps (quiet_window)."));
        assert!(
            ext_html
                .contains("Time subdivision: 2/48 intervals subdivided (52 substeps, 56 trials).")
        );
        assert!(ext_html.contains(
            "<h4>Static equilibrium comparison (frictionless)</h4><p>Status: available. Newton iterations: 9. Max displacement: 4.25 mm. RMS displacement: 0.001 mm.</p>"
        ));
        assert!(ext_html.contains("Frictionless comparison only; &lt;not&gt; fit."));

        let available_no_result = json!({
            "round": 3,
            "sim": {
                "settled": true,
                "self_contact": {"enabled": true},
                "static_equilibrium": {
                    "status": "available",
                    "no_claim": "Still not fit."
                },
                "gating": [],
                "advisory": []
            }
        });
        let avail_html = render(Path::new("."), &[available_no_result]).unwrap();
        assert!(avail_html.contains(
            "<h4>Static equilibrium comparison (frictionless)</h4><p>Status: available.</p><p class=\"muted\">Still not fit.</p>"
        ));

        let refused_eq = json!({
            "round": 4,
            "sim": {
                "settled": true,
                "self_contact": {"enabled": true},
                "body": {
                    "thumb_pose": {
                        "left_rotation_vector_rad": [0.1, 0.0, 0.0],
                        "right_rotation_vector_rad": [-0.1, 0.0, 0.0]
                    }
                },
                "static_equilibrium": {
                    "status": "refused",
                    "refusal": {"message": "history <law> refused"}
                },
                "gating": [],
                "advisory": []
            }
        });
        let ref_html = render(Path::new("."), &[refused_eq]).unwrap();
        assert!(ref_html.contains("Thumb pose (rad): L [0.1, 0, 0], R [-0.1, 0, 0]."));
        assert!(!ref_html.contains("hand-surface pairs audited"));
        assert!(ref_html.contains(
            "<h4>Static equilibrium comparison (frictionless)</h4><p>Status: refused — history &lt;law&gt; refused. Dynamic trajectory and flags remain authoritative.</p>"
        ));
    }
}
