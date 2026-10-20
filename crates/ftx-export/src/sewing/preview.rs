//! Two independently laid-out source-side frames. Display placement is not machine registration.

use ftx_core::{Refusal, RefusalCode, Vec2, curve::EDGE_DENSE};
use ftx_pattern::Pattern;
use serde_json::json;

use super::{SewOptions, SewPlan, bad};
use crate::{Ctx, ExportOptions, fnum, outline::cut_outline, xml_escape};

fn path(points: &[Vec2], closed: bool) -> String {
    let mut d = String::new();
    for (i, p) in points.iter().enumerate() {
        d.push_str(&format!(
            "{}{} {} ",
            if i == 0 { 'M' } else { 'L' },
            fnum(p.x, 6),
            fnum(p.y, 6)
        ));
    }
    if closed {
        d.push('Z');
    }
    d
}

/// Render one operation only after rebuilding and validating the entire material-aware job.
/// Exterior cuts, original seam lines, offset paths and every needle station are distinguished.
/// The two views are in millimetres; their independent display translations do not join the pieces.
/// No scripts, links, external fonts, device paths or hardware permission are generated.
///
/// # Errors
/// Any planning refusal; unknown operation; excessive preview size; non-finite geometry; internal
/// cuts/drills without explicit obstacle support; control characters in displayed labels.
pub fn svg(pattern: &Pattern, options: &SewOptions, operation: &str) -> Result<String, Refusal> {
    let plan = SewPlan::build(pattern, options)?;
    let seam = plan
        .seams
        .iter()
        .find(|s| s.pass.operation_id() == operation)
        .ok_or_else(|| bad("unknown sewing preview operation"))?;
    let budget = || {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            "sewing preview exceeds geometry budget",
        )
    };
    if seam.stations.a.len().saturating_add(seam.stations.b.len()) > 20_000 {
        return Err(budget());
    }
    for text in [
        operation,
        &seam.sides[0].reference,
        &seam.sides[1].reference,
    ] {
        if text.len() > 512
            || text
                .chars()
                .any(|c| c.is_control() || c == '\u{fffe}' || c == '\u{ffff}')
        {
            return Err(bad("invalid or excessive sewing preview label"));
        }
    }
    let ctx = Ctx::new(
        pattern,
        ExportOptions {
            sa: options.seam_allowance_mm,
            ..ExportOptions::default()
        },
    )?;
    let mut cuts = Vec::with_capacity(2);
    let mut sources = Vec::with_capacity(2);
    let mut bounds = Vec::with_capacity(2);
    for side in 0..2 {
        let reference = &seam.sides[side].reference;
        let (instance, _) = pattern.side(reference).map_err(|e| bad(e.to_string()))?;
        let piece = &pattern.doc.as_ref().expect("validated pattern").pieces[instance.piece_index];
        if instance.edges.len() > 10
            || instance
                .edges
                .iter()
                .any(|e| e.curve.as_ref().is_some_and(|c| c.controls().len() > 800))
        {
            return Err(budget());
        }
        if piece
            .internal
            .as_ref()
            .is_some_and(|m| m.iter().any(|m| m.kind.as_deref() == Some("cut")))
            || piece.drills.as_ref().is_some_and(|m| !m.is_empty())
        {
            return Err(bad(
                "sewing preview requires explicit internal cut/drill obstacle support",
            ));
        }
        let (cut, _) = cut_outline(instance, piece, None, ctx.sa, ctx.opts.mitre_limit)?;
        let (source, _) = pattern
            .side_polyline(reference)
            .map_err(|e| bad(e.to_string()))?;
        let stations = if side == 0 {
            &seam.stations.a
        } else {
            &seam.stations.b
        };
        let trace = seam
            .path
            .as_ref()
            .map_or(stations, |p| &p.polylines_mm[side]);
        let mut lo = Vec2::new(f64::INFINITY, f64::INFINITY);
        let mut hi = Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY);
        for p in cut.iter().chain(&source).chain(trace).chain(stations) {
            if !p.is_finite() {
                return Err(bad("non-finite sewing preview coordinate"));
            }
            lo.x = lo.x.min(p.x);
            lo.y = lo.y.min(p.y);
            hi.x = hi.x.max(p.x);
            hi.y = hi.y.max(p.y);
        }
        // Include the original full piece frame for an honest exterior view.
        if instance
            .outline(EDGE_DENSE)
            .map_err(|e| bad(e.to_string()))?
            .len()
            > 8192
        {
            return Err(budget());
        }
        if (hi - lo).norm() > 1e5 {
            return Err(budget());
        }
        bounds.push((lo, hi));
        cuts.push(cut);
        sources.push(source);
    }
    let column = bounds
        .iter()
        .map(|(lo, hi)| (hi.x - lo.x + 20.0).max(110.0))
        .collect::<Vec<_>>();
    let width = column.iter().sum::<f64>();
    let body_height = bounds
        .iter()
        .map(|(lo, hi)| hi.y - lo.y)
        .fold(0.0, f64::max);
    let foot_height = if seam.presser_foot.is_some() {
        10.0
    } else {
        0.0
    };
    let height = body_height + 68.0 + 5.0 * seam.runs.len() as f64 + foot_height;
    let (ws, hs) = (fnum(width, 6), fnum(height, 6));
    let mut metadata = json!({"schema":"ftx-sewing-preview/1","pattern_sha256":plan.pattern_sha256,
        "operation":seam.pass,"material_sha256":[seam.sides[0].material_sha256,seam.sides[1].material_sha256],
        "plan_schema":plan.schema,"hardware_execution_authorized":false,"physical_seam_quality_validated":false});
    if let Some(foot) = &seam.presser_foot {
        metadata["presser_foot"] = json!({"model":foot.model,"pressure_pa":foot.pressure_pa,
            "uncompressed_stack_mm":seam.stack_mm,"nominal_loaded_stack_mm":foot.nominal_stack_mm,
            "checked_loaded_stack_mm":foot.stack_mm,"layers":foot.layers,
            "acquisition_sha256":foot.materials.iter().map(|(h,r)| (h,&r.input_sha256))
                .collect::<std::collections::BTreeMap<_,_>>()});
    }
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{ws}mm\" height=\"{hs}mm\" viewBox=\"0 0 {ws} {hs}\">\n<title>Sewing operation: {}</title>\n<desc>Independent pattern frames. Planning review only, not a cutter file or robot trajectory. No physical seam qualification. Exterior clearance excludes notch incisions.</desc>\n<metadata>{}</metadata>\n<rect width=\"{ws}\" height=\"{hs}\" fill=\"white\"/>\n<defs><marker id=\"travel\" markerWidth=\"4\" markerHeight=\"4\" refX=\"4\" refY=\"2\" orient=\"auto\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L4 2 L0 4 Z\" fill=\"#087f8c\"/></marker></defs>\n<g font-family=\"sans-serif\" font-size=\"3\" fill=\"black\">\n<text x=\"5\" y=\"6\">Planning review only - NOT machine instructions</text>\n<text x=\"5\" y=\"12\">Black: exterior cut. Gray dash: original seam. Teal: stitch path; arrow: main pass.</text>\n<text x=\"5\" y=\"18\">Red dots: needle stations; red dash: between-stitch chords. Views are separate local frames.</text>\n",
        xml_escape(operation),
        xml_escape(&metadata.to_string())
    );
    let mut x = 0.0;
    for side in 0..2 {
        let (lo, hi) = bounds[side];
        let tx = x + 10.0 - lo.x;
        let ty = 34.0 + hi.y;
        let stations = if side == 0 {
            &seam.stations.a
        } else {
            &seam.stations.b
        };
        let mut trace = seam
            .path
            .as_ref()
            .map_or_else(|| stations.clone(), |p| p.polylines_mm[side].clone());
        if !seam.pass.direction.is_forward() {
            trace.reverse();
        }
        out.push_str(&format!("<text x=\"{}\" y=\"27\">{}: {}</text>\n<g id=\"side-{side}\" transform=\"translate({} {}) scale(1 -1)\" fill=\"none\" stroke-width=\"0.25\">\n<path class=\"cut-exterior\" d=\"{}\" stroke=\"black\"/>\n<path class=\"source-seam\" d=\"{}\" stroke=\"gray\" stroke-dasharray=\"2 1\"/>\n<path class=\"stitch-path\" d=\"{}\" stroke=\"#087f8c\" marker-end=\"url(#travel)\"/>\n<path class=\"stitch-chords\" d=\"{}\" stroke=\"#bb3344\" stroke-dasharray=\"0.7 0.7\" stroke-width=\"0.12\"/>\n",
            fnum(x+5.0,6), if side == 0 { "A" } else { "B" }, xml_escape(&seam.sides[side].reference),
            fnum(tx,6),fnum(ty,6),path(&cuts[side],true),path(&sources[side],false),path(&trace,false),path(stations,false)));
        for (i, p) in stations.iter().enumerate() {
            out.push_str(&format!("<circle class=\"needle\" data-station=\"{i}\" cx=\"{}\" cy=\"{}\" r=\"0.35\" fill=\"#bb3344\"><title>Station {i}</title></circle>\n", fnum(p.x,6), fnum(p.y,6)));
        }
        out.push_str("</g>\n");
        x += column[side];
    }
    let y = 44.0 + body_height;
    out.push_str(&format!("<text x=\"5\" y=\"{}\">Operation: {} | {} intervals | stack {} mm | continuous feed {} s</text>\n",
        fnum(y,6),xml_escape(operation),seam.stitch_intervals,fnum(seam.stack_mm,6),fnum(seam.feed_seconds,6)));
    if seam.runs.is_empty() {
        out.push_str(&format!("<text x=\"5\" y=\"{}\">Forward stations 0 to {}. No inter-operation handling or movement is generated.</text>\n",fnum(y+5.0,6),seam.stations.a.len()-1));
    }
    for (i, run) in seam.runs.iter().enumerate() {
        out.push_str(&format!("<text x=\"5\" y=\"{}\">Run {}: stations {} to {}, {} intervals (both material sides)</text>\n",
            fnum(y+5.0*(i+1) as f64,6),i,run.from_station,run.to_station,run.stitch_intervals));
    }
    if let Some(foot) = &seam.presser_foot {
        let fy = y + 5.0 * (seam.runs.len().max(1) + 1) as f64;
        out.push_str(&format!("<text class=\"loaded-stack\" x=\"5\" y=\"{}\">Foot pressure {} Pa | loaded nominal {} mm | checked stack {} mm (upper bounds where supplied)</text>\n<text x=\"5\" y=\"{}\">Prescribed independent-ply pressure; no contact/recovery model or machine setting.</text>\n",
            fnum(fy,6), fnum(foot.pressure_pa,6), fnum(foot.nominal_stack_mm,6),
            fnum(foot.stack_mm,6), fnum(fy+5.0,6)));
    }
    out.push_str("</g>\n</svg>\n");
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn inputs() -> (Pattern, SewOptions) {
        (
            Pattern::from_json(include_str!(
                "../../../../tests/fixtures/materials/synthetic-sewing-pattern.json"
            ))
            .unwrap(),
            serde_json::from_str(include_str!(
                "../../../../tests/fixtures/materials/synthetic-sewing-topstitch-options.json"
            ))
            .unwrap(),
        )
    }

    #[test]
    fn preview_preserves_local_paths_stations_and_operation_receipts() {
        let (p, o) = inputs();
        let s = svg(&p, &o, "topstitch").unwrap();
        assert_eq!(s, svg(&p, &o, "topstitch").unwrap());
        let root = crate::xml::parse(&s).unwrap();
        assert_eq!(root.get("width"), Some("220mm"));
        let nodes = root.descendants();
        assert_eq!(
            nodes
                .iter()
                .filter(|n| n.get("class") == Some("needle"))
                .count(),
            82
        );
        let paths = nodes
            .iter()
            .filter(|n| n.get("class") == Some("stitch-path"))
            .collect::<Vec<_>>();
        assert_eq!(paths[0].get("d"), Some("M17 100 L17 0 "));
        assert_eq!(paths[1].get("d"), Some("M4 100 L4 0 "));
        assert!(s.contains("ftx-sewing-plan/3"));
        assert!(s.contains("Run 0: stations 40 to 0, 40 intervals"));
        assert!(s.contains("NOT machine instructions"));
        assert!(!s.contains("<script") && !s.contains("href="));
    }

    #[test]
    fn preview_escapes_names_rechecks_all_operations_and_rejects_control_labels() {
        let (p, mut o) = inputs();
        let id = "<script>alert(1)</script>&\"";
        o.passes[1].id = Some(id.into());
        let s = svg(&p, &o, id).unwrap();
        assert!(!s.contains("<script>"));
        assert!(s.contains("&lt;script&gt;alert(1)&lt;/script&gt;&amp;&quot;"));
        o.passes[1].id = Some("bad\u{1}".into());
        assert!(svg(&p, &o, "bad\u{1}").is_err());
        o.passes[1].id = Some("topstitch".into());
        o.passes[0].stitches_per_minute = 99999.0;
        assert!(svg(&p, &o, "topstitch").is_err());
        let (p, o) = inputs();
        assert!(svg(&p, &o, "unknown").is_err());
        let mut raw: Value = p.raw().clone();
        raw["fabric"]["material"]["record"]["properties"]["thickness"]["value"] = json!(1.1);
        assert!(svg(&Pattern::new(raw), &o, "join").is_err());
    }
}
