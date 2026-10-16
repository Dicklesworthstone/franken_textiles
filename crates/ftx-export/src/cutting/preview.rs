//! Source-locked roll preview in millimetres. Groups distinguish outlines, drills and annotations;
//! this is not device code or permission to execute an unqualified cutting process.

use ftx_core::{Refusal, RefusalCode, Vec2};

use super::CutPlan;
use crate::{fnum, xml_escape};

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

/// Render one marker with exactly its planned stock dimensions and six-decimal millimetre paths.
/// The input is the computed cut plan, not a new geometry pass or a separately re-resolved material.
/// It contains no script, external resources or machine-specific settings. All labels are escaped.
///
/// # Errors
/// Missing roll index, invalid public geometry fields or dimensions after caller-side mutation.
pub fn svg(plan: &CutPlan, roll_index: usize) -> Result<String, Refusal> {
    let bad = || {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "invalid roll preview geometry",
        )
    };
    let roll = plan.rolls.get(roll_index).ok_or_else(bad)?;
    let w = roll.width_m * 1000.0;
    let h = roll.length_m * 1000.0;
    if !w.is_finite() || !h.is_finite() || w <= 0.0 || h <= 0.0 {
        return Err(bad());
    }
    let (ws, hs) = (fnum(w, 6), fnum(h, 6));
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{ws}mm\" height=\"{hs}mm\" viewBox=\"0 0 {ws} {hs}\">\n<title>{}</title>\n<desc>Single-ply one-way-grain planning preview. Not machine-qualified toolpaths.</desc>\n<metadata>pattern_sha256={}; material_sha256={}</metadata>\n<rect id=\"stock\" width=\"{ws}\" height=\"{hs}\" fill=\"white\" stroke=\"black\" stroke-width=\"0.2\"/>\n<defs><marker id=\"grain-arrow\" markerWidth=\"4\" markerHeight=\"4\" refX=\"4\" refY=\"2\" orient=\"auto\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L4 2 L0 4 Z\"/></marker></defs>\n<g transform=\"translate(0 {hs}) scale(1 -1)\" fill=\"none\" stroke=\"black\" stroke-width=\"0.15\">\n",
        xml_escape(&roll.material.material().name),
        xml_escape(&plan.pattern_sha256),
        xml_escape(roll.material.sha256())
    );
    for (index, p) in roll.placements.iter().enumerate() {
        if p.cut_mm.len() < 3
            || p.cut_mm.iter().chain(&p.grain_mm).any(|v| !v.is_finite())
            || p.internal_cut_mm
                .iter()
                .any(|(v, _)| v.len() < 2 || v.iter().any(|p| !p.is_finite()))
            || p.drills_mm
                .iter()
                .any(|(v, d)| !v.is_finite() || !d.is_finite() || *d <= 0.0)
            || p.bounds_mm.iter().any(|v| !v.is_finite())
        {
            return Err(bad());
        }
        out.push_str(&format!(
            "<g id=\"piece-{index}\" data-instance=\"{}\" data-copy=\"{}\"><path class=\"cut-exterior\" d=\"{}\"/>\n",
            xml_escape(&p.instance), p.copy, path(&p.cut_mm, true)
        ));
        for (v, closed) in &p.internal_cut_mm {
            out.push_str(&format!(
                "<path class=\"cut-interior\" d=\"{}\"/>\n",
                path(v, *closed)
            ));
        }
        for (v, diameter) in &p.drills_mm {
            out.push_str(&format!(
                "<circle class=\"drill\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n",
                fnum(v.x, 6),
                fnum(v.y, 6),
                fnum(diameter / 2.0, 6)
            ));
        }
        out.push_str(&format!("<path class=\"grain\" d=\"{}\" stroke-dasharray=\"2 1\" marker-end=\"url(#grain-arrow)\"/></g>\n",
            path(&p.grain_mm, false)));
    }
    out.push_str("</g>\n<g id=\"labels\" font-family=\"sans-serif\" font-size=\"3\">\n");
    for p in &roll.placements {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\">{} / copy {}</text>\n",
            fnum(p.bounds_mm[0] + 1.0, 6),
            fnum(h - p.bounds_mm[3] + 4.0, 6),
            xml_escape(&p.instance),
            p.copy
        ));
    }
    out.push_str("</g>\n</svg>\n");
    Ok(out)
}

/// Preview one individually identified hide/remnant from a complete finite-stock plan.
/// Coordinates are the plan's rigid cutting-axis frame in mm; boundaries/defects are NOT cut paths.
/// There are no external resources, scripts, device settings or implicit scaling of the stock.
///
/// # Errors
/// Invalid stock index, more than one million rendered points, or over 64 MiB of preview output.
pub fn stock_svg(plan: &super::stock::Plan, index: usize) -> Result<String, Refusal> {
    let marker = plan.markers().get(index).ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "finite-stock preview index is out of range",
        )
    })?;
    let mut count = marker.outline_mm.len();
    for n in marker
        .defects_mm
        .iter()
        .chain(&marker.occupied_mm)
        .map(Vec::len)
        .chain(marker.placements.iter().flat_map(|p| {
            [p.cut_mm.len(), 2, p.drills_mm.len()]
                .into_iter()
                .chain(p.internal_cut_mm.iter().map(|(v, _)| v.len()))
        }))
    {
        count = count.checked_add(n).ok_or_else(preview_budget)?;
    }
    if count > 1_000_000 {
        return Err(preview_budget());
    }
    let b = super::bbox(&marker.outline_mm)?;
    let (w, h) = (fnum(b[2] - b[0], 6), fnum(b[3] - b[1], 6));
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}mm\" height=\"{h}mm\" viewBox=\"{} {} {w} {h}\">\n<title>{}</title>\n<desc>Finite single-ply stock review. Stock boundary and defects are NOT cut paths. No machine calibration or execution authorization. Coordinate units: mm; one-way cutting axis +y.</desc>\n<metadata>material_sha256={}; stock_index={index}</metadata>\n<defs><marker id=\"grain-arrow\" markerWidth=\"4\" markerHeight=\"4\" refX=\"4\" refY=\"2\" orient=\"auto\" markerUnits=\"userSpaceOnUse\"><path d=\"M0 0 L4 2 L0 4 Z\"/></marker></defs>\n<g transform=\"scale(1 -1)\" stroke-width=\"0.15\">\n<path id=\"stock-boundary\" d=\"{}\" fill=\"white\" stroke=\"black\"/>\n<g id=\"stock-defects\" fill=\"#ddd\" stroke=\"#666\" stroke-dasharray=\"2 1\">\n",
        fnum(b[0], 6),
        fnum(-b[3], 6),
        xml_escape(&format!(
            "{} — {}",
            marker.id,
            marker.material.material().name
        )),
        xml_escape(marker.material.sha256()),
        path(&marker.outline_mm, true)
    );
    for defect in &marker.defects_mm {
        out.push_str(&format!("<path d=\"{}\"/>\n", path(defect, true)));
    }
    out.push_str(
        "</g>\n<g id=\"stock-occupied\" fill=\"#eee\" stroke=\"#999\" stroke-dasharray=\"1 1\">\n",
    );
    for occupied in &marker.occupied_mm {
        out.push_str(&format!("<path d=\"{}\"/>\n", path(occupied, true)));
    }
    out.push_str("</g>\n<g id=\"placements\" fill=\"none\" stroke=\"black\">\n");
    for (i, p) in marker.placements.iter().enumerate() {
        out.push_str(&format!(
            "<g id=\"piece-{i}\" data-instance=\"{}\" data-copy=\"{}\"><path class=\"cut-exterior\" d=\"{}\"/>\n",
            xml_escape(&p.instance), p.copy, path(&p.cut_mm, true)
        ));
        for (v, closed) in &p.internal_cut_mm {
            out.push_str(&format!(
                "<path class=\"cut-interior\" d=\"{}\"/>\n",
                path(v, *closed)
            ));
        }
        for (v, diameter) in &p.drills_mm {
            out.push_str(&format!(
                "<circle class=\"drill\" cx=\"{}\" cy=\"{}\" r=\"{}\"/>\n",
                fnum(v.x, 6),
                fnum(v.y, 6),
                fnum(diameter / 2.0, 6)
            ));
        }
        out.push_str(&format!(
            "<path class=\"grain\" d=\"{}\" stroke-dasharray=\"2 1\" marker-end=\"url(#grain-arrow)\"/></g>\n",
            path(&p.grain_mm, false)
        ));
        if out.len() > 64 * 1024 * 1024 {
            return Err(preview_budget());
        }
    }
    out.push_str("</g></g>\n<g id=\"labels\" font-family=\"sans-serif\" font-size=\"3\">\n");
    for p in &marker.placements {
        out.push_str(&format!(
            "<text x=\"{}\" y=\"{}\">{} / copy {}</text>\n",
            fnum(p.bounds_mm[0] + 1.0, 6),
            fnum(-p.bounds_mm[3] + 4.0, 6),
            xml_escape(&p.instance),
            p.copy
        ));
    }
    out.push_str("</g>\n</svg>\n");
    if out.len() > 64 * 1024 * 1024 {
        return Err(preview_budget());
    }
    Ok(out)
}
fn preview_budget() -> Refusal {
    Refusal::new(
        RefusalCode::BudgetExceeded,
        "finite-stock preview exceeds output budget",
    )
}
