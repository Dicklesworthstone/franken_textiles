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
            out.push_str(&crate::two_start::review_html(sim));
            let _ = write!(
                out,
                "<h3>Drape prediction</h3><p>Settled: {}. Self-contact enabled: {}.</p>",
                cell(&sim["settled"]),
                cell(&sim["self_contact"]["enabled"])
            );
            out.push_str("<strong>Gating flags</strong>");
            list(&mut out, &sim["gating"]);
            out.push_str("<strong>Advisory flags</strong>");
            list(&mut out, &sim["advisory"]);
            if let Some(dir) = round_dir(&entry["dir"]) {
                for (name, label) in [
                    ("fold_viewer.html", "Open drape viewer"),
                    ("sim_report.json", "Full drape report"),
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
            "viewer":"javascript:alert(1)","images":["https://bad.test/track"],"exports":null});
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
}
