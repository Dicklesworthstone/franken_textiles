//! Human hand-knitting pure-vector CYC chart SVG & PDF exporter (`FTX-K07`) with dedicated
//! acceptance checks (`check_hand_knit_svg`, `check_hand_knit_pdf`).
//!
//! Every Craft Yarn Council (CYC) stitch symbol is drawn as explicit 2D vector geometry (lines,
//! polylines, filled circles) with ZERO Unicode symbol glyphs so output renders crisply in
//! WinAnsi `/Helvetica` PDF viewers and standard SVG renderers without `transform` attributes.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode, Vec2};
use ftx_pattern::knit::{Bed, KnitDoc, LoopKind};

use crate::checks::{CheckItem, pdf_objects};
use crate::pdf::{PdfDoc, TextAnchor};
use crate::svg::SVG_NAMESPACE;
use crate::text::{ascii_text, fnum, xml_escape};
use crate::xml::parse;

/// Complete human hand-knitting export bundle.
#[derive(Clone, Debug)]
pub struct HandKnitExportBundle {
    /// Pure-vector mm-unit SVG chart (`svg.no_transform`, 100 mm calibration square, CYC symbols).
    pub svg_text: String,
    /// Printable vector PDF document (built with deterministic `created` timestamp).
    pub pdf_bytes: Vec<u8>,
    /// Row-by-row written ASCII knitting instructions.
    pub instructions_text: String,
    /// SHA-256 of `svg_text`.
    pub svg_sha256: String,
    /// SHA-256 of `pdf_bytes`.
    pub pdf_sha256: String,
}

/// Exports a validated [`KnitDoc`] as a pure-vector CYC hand-knit SVG, PDF, and written row guide.
///
/// `created` should follow `YYYY-MM-DDTHH:MM` for deterministic PDF digests.
///
/// # Errors
/// If `doc.validate()` fails.
pub fn export_hand_knit_document(
    doc: &KnitDoc,
    created: &str,
) -> Result<HandKnitExportBundle, Refusal> {
    doc.validate()?;
    if doc.wearer_age_class.is_child() && doc.has_hood_or_neck_drawstring {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "EN 14682 child-safety refusal: hand-knit chart export refuses hood or neck drawstrings on child/infant knitwear",
        )
        .repair("remove hood/neck drawstrings and replace with ribbed shaping or concealed snaps"));
    }
    if doc.wearer_age_class.is_infant() && doc.short_row_turns.iter().any(|turn| !turn.anchored) {
        return Err(Refusal::new(
            RefusalCode::MaterialProcessHazard,
            "infant-safety refusal: hand-knit chart export refuses unanchored short-row eyelet holes on infant (<3 yr) knitwear",
        )
        .repair("anchor every short-row turn with a wrap-and-turn or German short-row double stitch"));
    }

    let mut min_w = i32::MAX;
    let mut max_w = i32::MIN;
    let mut max_c = 0u32;
    // A course on a tubular garment contains separate front and back stitches at the same wale.
    // Bed identity is therefore part of the chart address, never a display-only annotation.
    let mut beds = BTreeSet::new();
    let mut grid = BTreeMap::new();
    for st in &doc.stitches {
        min_w = min_w.min(st.wale);
        max_w = max_w.max(st.wale);
        max_c = max_c.max(st.course);
        beds.insert(st.bed);
        if grid.insert((st.bed, st.course, st.wale), st).is_some() {
            return Err(Refusal::new(
                RefusalCode::InvalidDocument,
                format!(
                    "hand-knit chart has multiple stitches on {} at course {}, wale {}; no stitch may be overwritten",
                    bed_label(st.bed), st.course, st.wale
                ),
            )
            .repair("give distinct stitch operations distinct courses or needle-bed addresses"));
        }
    }
    let cols_i64 = i64::from(max_w) - i64::from(min_w) + 1;
    let rows_u64 = u64::from(max_c) + 1;
    if cols_i64 <= 0
        || cols_i64 > 2_000
        || rows_u64 > 5_000
        || (cols_i64 as u64)
            .saturating_mul(rows_u64)
            .saturating_mul(beds.len() as u64)
            > 500_000
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "hand-knit chart grid {cols_i64} x {rows_u64} on {} beds exceeds vector export budget (max 2000 cols, 5000 rows, 500000 total cells)",
                beds.len()
            ),
        ));
    }
    let cols = cols_i64 as usize;
    let rows = rows_u64 as usize;

    // Layout geometry in mm (y down on page)
    let margin = 15.0;
    let header_h = 28.0;
    let cell = (160.0 / (cols.max(1) as f64)).clamp(3.0, 6.0);
    let chart_w = cols as f64 * cell;
    let chart_h = rows as f64 * cell;
    let panel_gap = 14.0;
    let all_charts_h = chart_h * beds.len() as f64 + panel_gap * (beds.len() - 1) as f64;
    let cal_block_w = 112.0; // holds 100 mm calibration square + legend
    let page_w = (margin * 2.0 + chart_w.max(cal_block_w) + 18.0).max(210.0);
    let page_h = (margin * 2.0 + header_h + chart_h + 125.0).max(297.0);
    let svg_h = (margin * 2.0 + header_h + all_charts_h + 125.0).max(297.0);
    let chart_x = margin + 10.0;
    let chart_y = margin + header_h;

    // 1. Build pure-vector SVG (NO transform attributes!)
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="{SVG_NAMESPACE}" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#,
        w = fnum(page_w, 2),
        h = fnum(svg_h, 2)
    );
    let _ = writeln!(
        svg,
        r#"<rect x="0" y="0" width="{w}" height="{h}" fill="white"/>"#,
        w = fnum(page_w, 2),
        h = fnum(svg_h, 2)
    );
    let title = ascii_text(&format!("FrankenTextiles Hand-Knit Chart: {}", doc.name));
    let gauge_line = ascii_text(&format!(
        "Gauge: {:.1} wales x {:.1} courses per 100mm | Grid per bed: {} x {} | Wearer: {:?}",
        doc.gauge.wales_per_100mm, doc.gauge.courses_per_100mm, cols, rows, doc.wearer_age_class
    ));
    let _ = writeln!(
        svg,
        r#"<g id="hand-knit-header"><text x="{x}" y="{y1}" font-family="Helvetica, sans-serif" font-size="5" font-weight="bold">{t}</text><text x="{x}" y="{y2}" font-family="Helvetica, sans-serif" font-size="3.2">{g}</text></g>"#,
        x = fnum(margin, 2),
        y1 = fnum(margin + 6.0, 2),
        y2 = fnum(margin + 13.0, 2),
        t = xml_escape(&title),
        g = xml_escape(&gauge_line)
    );

    // Chart grid & CYC pure vector symbols
    svg.push_str(r#"<g id="hand-knit-chart">"#);
    for (panel, &bed) in beds.iter().enumerate() {
        let panel_y = chart_y + panel as f64 * (chart_h + panel_gap);
        let _ = write!(
            svg,
            r#"<g id="hand-knit-chart-{bed_id}"><text x="{x}" y="{y}" font-family="Helvetica, sans-serif" font-size="3.5" font-weight="bold">{label} - wales {min_w} to {max_w}</text>"#,
            bed_id = bed.knitout_prefix(),
            x = fnum(chart_x, 2),
            y = fnum(panel_y - 4.0, 2),
            label = bed_label(bed),
        );
        for r in 0..rows {
            let course = (rows - 1 - r) as u32;
            let y0 = panel_y + r as f64 * cell;
            for c in 0..cols {
                let wale = min_w + c as i32;
                let x0 = chart_x + c as f64 * cell;
                if let Some(stitch) = grid.get(&(bed, course, wale)) {
                    let _ = write!(
                        svg,
                        r##"<g id="hand-knit-stitch-{id}" data-bed="{bed}" data-course="{course}" data-wale="{wale}"><rect x="{x}" y="{y}" width="{s}" height="{s}" fill="none" stroke="#444444" stroke-width="0.22"/>"##,
                        id = stitch.id,
                        bed = bed.knitout_prefix(),
                        x = fnum(x0, 2),
                        y = fnum(y0, 2),
                        s = fnum(cell, 2)
                    );
                    emit_cyc_symbol_svg(&mut svg, stitch.kind, x0, y0, cell);
                    svg.push_str("</g>");
                } else {
                    // No-stitch placeholder (shaded gray cell per CYC standard)
                    let _ = write!(
                        svg,
                        r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" fill="#d9d9d9" stroke="#888888" stroke-width="0.18"/>"##,
                        x = fnum(x0, 2),
                        y = fnum(y0, 2),
                        s = fnum(cell, 2)
                    );
                }
            }
            // Row index every 5 rows + row 1
            let row_num = course + 1;
            if row_num == 1 || row_num.is_multiple_of(5) {
                let _ = write!(
                    svg,
                    r#"<text x="{x}" y="{y}" font-family="Helvetica, sans-serif" font-size="2.5">{row_num}</text>"#,
                    x = fnum(chart_x + chart_w + 2.0, 2),
                    y = fnum(y0 + cell * 0.72, 2)
                );
            }
        }
        svg.push_str("</g>\n");
    }
    svg.push_str("</g>\n");

    // 100 mm x 100 mm physical print calibration square + CYC vector legend
    let cal_x = margin;
    let cal_y = chart_y + all_charts_h + 12.0;
    let _ = writeln!(
        svg,
        r##"<g id="calibration-square-100mm"><rect x="{x}" y="{y}" width="100" height="100" fill="none" stroke="#111111" stroke-width="0.4"/><text x="{tx}" y="{ty}" font-family="Helvetica, sans-serif" font-size="3.5">100 mm x 100 mm Print Scale Verification Square</text></g>"##,
        x = fnum(cal_x, 2),
        y = fnum(cal_y, 2),
        tx = fnum(cal_x + 4.0, 2),
        ty = fnum(cal_y + 8.0, 2)
    );

    let leg_x = cal_x + 108.0;
    svg.push_str(r#"<g id="cyc-legend">"#);
    for (idx, (kind, label)) in [
        (LoopKind::Knit, "Knit (RS) / Purl (WS)"),
        (LoopKind::Purl, "Purl (RS) / Knit (WS)"),
        (LoopKind::Decrease, "K2tog / Decrease"),
        (LoopKind::Increase, "M1 / Increase"),
        (LoopKind::Tuck, "Tuck / Yarn Over"),
        (LoopKind::Slip, "Slip stitch"),
        (LoopKind::Cable, "Cable crossing"),
    ]
    .iter()
    .enumerate()
    {
        let ly = cal_y + idx as f64 * 8.0;
        let _ = write!(
            svg,
            r##"<rect x="{x}" y="{y}" width="5" height="5" fill="none" stroke="#333333" stroke-width="0.25"/>"##,
            x = fnum(leg_x, 2),
            y = fnum(ly, 2)
        );
        emit_cyc_symbol_svg(&mut svg, *kind, leg_x, ly, 5.0);
        let _ = write!(
            svg,
            r#"<text x="{tx}" y="{ty}" font-family="Helvetica, sans-serif" font-size="3">{lbl}</text>"#,
            tx = fnum(leg_x + 7.0, 2),
            ty = fnum(ly + 3.8, 2),
            lbl = xml_escape(label)
        );
    }
    svg.push_str("</g>\n</svg>\n");

    // 2. Build vector PDF using ftx-export's native PdfDoc/Canvas engine
    let mut pdf = PdfDoc::new(
        vec![
            "Hand-Knit Chart".to_string(),
            "Calibration Grid".to_string(),
        ],
        &[],
        title.clone(),
    );
    for &bed in &beds {
        let mut cv = pdf.canvas();
        cv.fill_gray(0.0);
        cv.stroke_gray(0.15);
        cv.text(
            margin,
            margin + 5.0,
            &title,
            5.0,
            TextAnchor::Start,
            true,
            None,
        );

        cv.text(
            chart_x,
            chart_y - 4.0,
            &format!("{} - wales {min_w} to {max_w}", bed_label(bed)),
            3.5,
            TextAnchor::Start,
            true,
            None,
        );
        cv.text(
            margin,
            margin + 12.0,
            &gauge_line,
            3.0,
            TextAnchor::Start,
            false,
            None,
        );

        cv.width(0.22);
        for r in 0..rows {
            let course = (rows - 1 - r) as u32;
            let y0 = chart_y + r as f64 * cell;
            for c in 0..cols {
                let wale = min_w + c as i32;
                let x0 = chart_x + c as f64 * cell;
                let box_pts = [
                    Vec2::new(x0, y0),
                    Vec2::new(x0 + cell, y0),
                    Vec2::new(x0 + cell, y0 + cell),
                    Vec2::new(x0, y0 + cell),
                ];
                if let Some(stitch) = grid.get(&(bed, course, wale)) {
                    cv.stroke_gray(0.25);
                    cv.path(&box_pts, true);
                    cv.raw("S");
                    emit_cyc_symbol_pdf(&mut cv, stitch.kind, x0, y0, cell);
                } else {
                    cv.fill_gray(0.85);
                    cv.path(&box_pts, true);
                    cv.raw("f");
                }
            }
            let row_num = course + 1;
            if row_num == 1 || row_num.is_multiple_of(5) {
                cv.fill_gray(0.0);
                cv.text(
                    chart_x + chart_w + 2.0,
                    y0 + cell * 0.72,
                    &row_num.to_string(),
                    2.4,
                    TextAnchor::Start,
                    false,
                    None,
                );
            }
        }
        // 100 mm calibration square in PDF
        let cal_y = chart_y + chart_h + 12.0;
        cv.width(0.4);
        cv.stroke_gray(0.0);
        cv.path(
            &[
                Vec2::new(cal_x, cal_y),
                Vec2::new(cal_x + 100.0, cal_y),
                Vec2::new(cal_x + 100.0, cal_y + 100.0),
                Vec2::new(cal_x, cal_y + 100.0),
            ],
            true,
        );
        cv.raw("S");
        cv.fill_gray(0.0);
        cv.text(
            cal_x + 4.0,
            cal_y + 8.0,
            "100 mm x 100 mm Print Scale Verification Square",
            3.2,
            TextAnchor::Start,
            true,
            None,
        );
        pdf.add_page(page_w, page_h, cv, String::new());
    }
    let pdf_bytes = pdf.write(created);

    // 3. Build deterministic ASCII written row instructions
    let mut instr = String::new();
    let _ = writeln!(instr, "# {title}");
    let _ = writeln!(instr, "{gauge_line}\n");
    let _ = writeln!(
        instr,
        "Each bed has a separate chart panel and PDF page. The same course number on different beds belongs to the same source course, not successive rows."
    );
    let _ = writeln!(
        instr,
        "The guide below lists source stitch operations in increasing wale order. Bed names do not imply right/wrong sides or a working direction; follow the pattern's flat or circular construction and chart legend. Gaps are no-stitch cells.\n"
    );
    if let Some(finishing) = doc.metadata.get("manual_finishing") {
        let _ = writeln!(
            instr,
            "## Construction and finishing\n\n{}\n",
            ascii_text(finishing)
        );
    }
    if let Some(start) = doc
        .metadata
        .get("shoulder_start_course")
        .and_then(|value| value.parse::<u32>().ok())
        .and_then(|course| course.checked_add(1))
    {
        let _ = writeln!(
            instr,
            "Start separate shoulder sections at chart course {start}. Keep the neckline loops on hold; use a separate yarn supply for each shoulder."
        );
        for (side, key) in [
            ("Left", "left_shoulder_carrier"),
            ("Right", "right_shoulder_carrier"),
        ] {
            if let Some(carrier) = doc.metadata.get(key) {
                let direction_key = format!("shoulder_initial_direction_{carrier}");
                let direction = match doc.metadata.get(&direction_key).map(String::as_str) {
                    Some("plus") => "increasing wale order",
                    Some("minus") => "decreasing wale order",
                    _ => "the direction given in the source pattern",
                };
                let _ = writeln!(
                    instr,
                    "{side} shoulder: carrier {}, first pass in {direction}; alternate direction on subsequent courses.",
                    ascii_text(carrier)
                );
            }
        }
        if let Some(ids) = doc.metadata.get("held_neck_stitch_ids") {
            let _ = writeln!(
                instr,
                "Held neckline source stitch IDs: {}.\n",
                ascii_text(ids)
            );
        }
    }
    for course in 0..=max_c {
        for &bed in &beds {
            let mut counts: Vec<(LoopKind, usize)> = Vec::new();
            for wale in min_w..=max_w {
                if let Some(stitch) = grid.get(&(bed, course, wale)) {
                    let kind = stitch.kind;
                    if let Some(last) = counts.last_mut()
                        && last.0 == kind
                    {
                        last.1 += 1;
                        continue;
                    }
                    counts.push((kind, 1));
                }
            }
            if counts.is_empty() {
                continue;
            }
            let tokens: Vec<String> = counts
                .into_iter()
                .map(|(k, n)| format!("{}{n}", cyc_abbrev(k)))
                .collect();
            let _ = writeln!(
                instr,
                "Course {} - {}: {}",
                course + 1,
                bed_label(bed),
                tokens.join(", ")
            );
        }
    }

    let svg_sha256 = sha256_hex(svg.as_bytes());
    let pdf_sha256 = sha256_hex(&pdf_bytes);
    Ok(HandKnitExportBundle {
        svg_text: svg,
        pdf_bytes,
        instructions_text: instr,
        svg_sha256,
        pdf_sha256,
    })
}

const fn bed_label(bed: Bed) -> &'static str {
    match bed {
        Bed::Front => "Front bed",
        Bed::Back => "Back bed",
        Bed::FrontSlider => "Front slider",
        Bed::BackSlider => "Back slider",
    }
}

/// Dedicated acceptance check for human hand-knit CYC vector SVG outputs (`FTX-K07`).
#[must_use]
pub fn check_hand_knit_svg(text: &str, file: &str) -> Vec<CheckItem> {
    let parsed = parse(text);
    let root = parsed.as_ref();
    let attr = |k: &str| root.and_then(|r| r.get(k)).unwrap_or("").to_string();
    let (w, h, vb) = (attr("width"), attr("height"), attr("viewBox"));
    let mut mm_ok = root.is_some() && w.ends_with("mm") && h.ends_with("mm");
    if mm_ok {
        let v: Option<Vec<f64>> = vb.split_whitespace().map(|q| q.parse().ok()).collect();
        let num = |s: &str| s[..s.len() - 2].trim().parse::<f64>().ok();
        mm_ok = match (v, num(&w), num(&h)) {
            (Some(v), Some(wv), Some(hv)) => {
                v.len() == 4
                    && wv.is_finite()
                    && hv.is_finite()
                    && wv > 0.0
                    && hv > 0.0
                    && v[0] == 0.0
                    && v[1] == 0.0
                    && (v[2] - wv).abs() < 1e-6
                    && (v[3] - hv).abs() < 1e-6
            }
            _ => false,
        };
    }
    let no_transform_ok = root.is_some_and(|r| {
        !r.descendants()
            .into_iter()
            .any(|el| el.get("transform").is_some())
    });
    vec![
        CheckItem {
            check: "hand_knit_svg.no_transform".to_string(),
            file: file.to_string(),
            ok: no_transform_ok,
            detail: String::new(),
        },
        CheckItem {
            check: "hand_knit_svg.mm_units_viewbox".to_string(),
            file: file.to_string(),
            ok: mm_ok,
            detail: format!("width={w} height={h} viewBox={vb}"),
        },
        CheckItem {
            check: "hand_knit_svg.chart_and_calibration".to_string(),
            file: file.to_string(),
            ok: text.contains(r#"id="hand-knit-chart""#)
                && text.contains(r#"id="calibration-square-100mm""#)
                && text.contains(r#"id="cyc-legend""#),
            detail: String::new(),
        },
        CheckItem {
            check: "hand_knit_svg.ascii_safe".to_string(),
            file: file.to_string(),
            ok: text.is_ascii(),
            detail: String::new(),
        },
    ]
}

/// Dedicated acceptance check for human hand-knit vector PDF outputs (`FTX-K07`).
#[must_use]
pub fn check_hand_knit_pdf(bytes: &[u8], file: &str) -> Vec<CheckItem> {
    let header_ok = bytes.starts_with(b"%PDF-1.");
    let eof_ok = bytes.ends_with(b"%%EOF\n") || bytes.ends_with(b"%%EOF\r\n");
    let objs = pdf_objects(bytes);
    let has_page = objs.values().any(|(dict, _)| {
        let s = String::from_utf8_lossy(dict);
        s.contains("/Type /Page") && !s.contains("/Type /Pages")
    });
    let has_decoded_stream = objs
        .values()
        .any(|(_, stream)| stream.as_ref().is_some_and(|st| !st.is_empty()));
    let has_font = objs
        .values()
        .any(|(dict, _)| String::from_utf8_lossy(dict).contains("/BaseFont /Helvetica"));
    vec![
        CheckItem {
            check: "hand_knit_pdf.header_and_eof".to_string(),
            file: file.to_string(),
            ok: header_ok && eof_ok,
            detail: format!("bytes={}", bytes.len()),
        },
        CheckItem {
            check: "hand_knit_pdf.pages_and_winansi_font".to_string(),
            file: file.to_string(),
            ok: has_page && has_decoded_stream && has_font,
            detail: format!("objects={}", objs.len()),
        },
    ]
}

const fn cyc_abbrev(kind: LoopKind) -> &'static str {
    match kind {
        LoopKind::Knit => "k",
        LoopKind::Purl => "p",
        LoopKind::Tuck => "yo/tuck",
        LoopKind::Slip => "sl",
        LoopKind::CastOn => "co",
        LoopKind::BindOff => "bo",
        LoopKind::Increase => "inc",
        LoopKind::Decrease => "k2tog",
        LoopKind::Split => "split",
        LoopKind::Cable => "cbl",
    }
}

fn emit_cyc_symbol_svg(out: &mut String, kind: LoopKind, x: f64, y: f64, s: f64) {
    let cx = x + 0.5 * s;
    let cy = y + 0.5 * s;
    match kind {
        LoopKind::Knit | LoopKind::CastOn | LoopKind::BindOff => {}
        LoopKind::Purl => {
            let _ = write!(
                out,
                r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="#111111"/>"##,
                cx = fnum(cx, 2),
                cy = fnum(cy, 2),
                r = fnum(s * 0.16, 2)
            );
        }
        LoopKind::Tuck => {
            let _ = write!(
                out,
                r##"<circle cx="{cx}" cy="{cy}" r="{r}" fill="none" stroke="#111111" stroke-width="0.28"/>"##,
                cx = fnum(cx, 2),
                cy = fnum(cy, 2),
                r = fnum(s * 0.24, 2)
            );
        }
        LoopKind::Decrease => {
            let _ = write!(
                out,
                r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#111111" stroke-width="0.32"/>"##,
                x1 = fnum(x + 0.2 * s, 2),
                y1 = fnum(y + 0.82 * s, 2),
                x2 = fnum(x + 0.8 * s, 2),
                y2 = fnum(y + 0.18 * s, 2)
            );
        }
        LoopKind::Increase | LoopKind::Split => {
            let _ = write!(
                out,
                r##"<polyline points="{x1},{y1} {cx},{y2} {x2},{y1}" fill="none" stroke="#111111" stroke-width="0.3"/>"##,
                x1 = fnum(x + 0.2 * s, 2),
                y1 = fnum(y + 0.22 * s, 2),
                cx = fnum(cx, 2),
                y2 = fnum(y + 0.82 * s, 2),
                x2 = fnum(x + 0.8 * s, 2)
            );
        }
        LoopKind::Slip => {
            let _ = write!(
                out,
                r##"<polyline points="{x1},{y1} {cx},{y2} {x2},{y1}" fill="none" stroke="#111111" stroke-width="0.28"/>"##,
                x1 = fnum(x + 0.22 * s, 2),
                y1 = fnum(y + 0.78 * s, 2),
                cx = fnum(cx, 2),
                y2 = fnum(y + 0.22 * s, 2),
                x2 = fnum(x + 0.78 * s, 2)
            );
        }
        LoopKind::Cable => {
            let _ = write!(
                out,
                r##"<line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}" stroke="#111111" stroke-width="0.3"/><line x1="{x1}" y1="{y2}" x2="{x2}" y2="{y1}" stroke="#111111" stroke-width="0.3"/>"##,
                x1 = fnum(x + 0.18 * s, 2),
                y1 = fnum(y + 0.18 * s, 2),
                x2 = fnum(x + 0.82 * s, 2),
                y2 = fnum(y + 0.82 * s, 2)
            );
        }
    }
}

fn emit_cyc_symbol_pdf(cv: &mut crate::pdf::Canvas, kind: LoopKind, x: f64, y: f64, s: f64) {
    let cx = x + 0.5 * s;
    let cy = y + 0.5 * s;
    cv.stroke_gray(0.0);
    cv.fill_gray(0.0);
    match kind {
        LoopKind::Knit | LoopKind::CastOn | LoopKind::BindOff => {}
        LoopKind::Purl => {
            let r = s * 0.16;
            cv.path(
                &[
                    Vec2::new(cx - r, cy),
                    Vec2::new(cx, cy - r),
                    Vec2::new(cx + r, cy),
                    Vec2::new(cx, cy + r),
                ],
                true,
            );
            cv.raw("f");
        }
        LoopKind::Tuck => {
            let r = s * 0.24;
            cv.path(
                &[
                    Vec2::new(cx - r, cy),
                    Vec2::new(cx, cy - r),
                    Vec2::new(cx + r, cy),
                    Vec2::new(cx, cy + r),
                ],
                true,
            );
            cv.raw("S");
        }
        LoopKind::Decrease => {
            cv.path(
                &[
                    Vec2::new(x + 0.2 * s, y + 0.82 * s),
                    Vec2::new(x + 0.8 * s, y + 0.18 * s),
                ],
                false,
            );
            cv.raw("S");
        }
        LoopKind::Increase | LoopKind::Split => {
            cv.path(
                &[
                    Vec2::new(x + 0.2 * s, y + 0.22 * s),
                    Vec2::new(cx, y + 0.82 * s),
                    Vec2::new(x + 0.8 * s, y + 0.22 * s),
                ],
                false,
            );
            cv.raw("S");
        }
        LoopKind::Slip => {
            cv.path(
                &[
                    Vec2::new(x + 0.22 * s, y + 0.78 * s),
                    Vec2::new(cx, y + 0.22 * s),
                    Vec2::new(x + 0.78 * s, y + 0.78 * s),
                ],
                false,
            );
            cv.raw("S");
        }
        LoopKind::Cable => {
            cv.path(
                &[
                    Vec2::new(x + 0.18 * s, y + 0.18 * s),
                    Vec2::new(x + 0.82 * s, y + 0.82 * s),
                ],
                false,
            );
            cv.raw("S");
            cv.path(
                &[
                    Vec2::new(x + 0.18 * s, y + 0.82 * s),
                    Vec2::new(x + 0.82 * s, y + 0.18 * s),
                ],
                false,
            );
            cv.raw("S");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knit::{
        export_brother_ayab, export_calibration_swatch_kit, export_kniterate_kc,
        refuse_proprietary_binary_target,
    };
    use ftx_core::RefusalCode;
    use ftx_pattern::knit::{
        CarrierId, GaugeSpec, MachineProfile, ParametricPanelSpec, WearerAgeClass,
        compile_knit_doc, draft_calibration_swatch, draft_shaped_panel,
    };

    #[test]
    fn open_neck_instructions_include_the_actual_held_loops_and_shoulder_carriers() {
        let profile = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 24.0,
            courses_per_100mm: 32.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(180.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let doc = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "open_neck_panel".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                hem_width_mm: 180.0,
                chest_width_mm: 180.0,
                length_mm: 120.0,
                armhole_depth_mm: 40.0,
                armhole_inset_total_mm: 30.0,
                neck_width_mm: 60.0,
                neck_depth_mm: 20.0,
                rib_hem_courses: 4,
                cam_setting: 8.0,
                yarn_id: "test-yarn".to_string(),
                carrier_id: CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &profile,
        )
        .unwrap();
        let exported = export_hand_knit_document(&doc, "2026-11-27T08:00").unwrap();
        let guide = &exported.instructions_text;
        assert!(guide.contains(&doc.metadata["manual_finishing"]));
        let start: u32 = doc.metadata["shoulder_start_course"].parse().unwrap();
        assert!(guide.contains(&format!(
            "Start separate shoulder sections at chart course {}",
            start + 1
        )));
        assert!(guide.contains(&format!(
            "Left shoulder: carrier {}, first pass in increasing wale order",
            doc.metadata["left_shoulder_carrier"]
        )));
        assert!(guide.contains(&format!(
            "Right shoulder: carrier {}, first pass in decreasing wale order",
            doc.metadata["right_shoulder_carrier"]
        )));
        let held_ids = &doc.metadata["held_neck_stitch_ids"];
        assert!(guide.contains(&format!("Held neckline source stitch IDs: {held_ids}.")));
        for id in held_ids.split(',').map(|id| id.parse::<u32>().unwrap()) {
            let source = doc.stitches.iter().find(|stitch| stitch.id == id).unwrap();
            assert_eq!(source.course, start - 1);
            assert!(
                doc.stitches
                    .iter()
                    .all(|stitch| stitch.parents.iter().all(|parent| parent.stitch_id != id))
            );
            assert!(
                exported
                    .svg_text
                    .contains(&format!("id=\"hand-knit-stitch-{id}\""))
            );
        }
    }

    #[test]
    fn front_and_back_stitches_survive_each_chart_and_written_course() {
        let profile = MachineProfile::kniterate_7e();
        let mut doc = draft_calibration_swatch(&profile, "test-yarn", 4, 2, &[6.0]).unwrap();
        let id_offset = doc.stitches.len() as u32;
        let back: Vec<_> = doc
            .stitches
            .iter()
            .cloned()
            .map(|mut stitch| {
                stitch.id += id_offset;
                stitch.bed = Bed::Back;
                for parent in &mut stitch.parents {
                    parent.stitch_id += id_offset;
                }
                if stitch.kind == LoopKind::Knit {
                    stitch.kind = LoopKind::Purl;
                }
                stitch
            })
            .collect();
        doc.stitches.extend(back);
        let exported = export_hand_knit_document(&doc, "2026-11-27T08:00").unwrap();

        let root = parse(&exported.svg_text).unwrap();
        let descendants = root.descendants();
        let cells: Vec<_> = descendants
            .iter()
            .filter(|element| element.get("data-bed").is_some())
            .collect();
        assert_eq!(cells.len(), doc.stitches.len());
        for stitch in &doc.stitches {
            let id = format!("hand-knit-stitch-{}", stitch.id);
            let cell = cells
                .iter()
                .find(|element| element.get("id") == Some(id.as_str()))
                .expect("every source stitch has its own vector cell");
            assert_eq!(cell.get("data-bed"), Some(stitch.bed.knitout_prefix()));
            assert_eq!(
                cell.get("data-course").unwrap().parse::<u32>().unwrap(),
                stitch.course
            );
            assert_eq!(
                cell.get("data-wale").unwrap().parse::<i32>().unwrap(),
                stitch.wale
            );
            if matches!(stitch.kind, LoopKind::Knit | LoopKind::Purl) {
                assert_eq!(
                    cell.children.iter().any(|element| element.name == "circle"),
                    stitch.kind == LoopKind::Purl,
                    "opposite-bed symbol must not replace this stitch"
                );
            }
        }
        assert!(exported.svg_text.contains("Front bed - wales 0 to 3"));
        assert!(exported.svg_text.contains("Back bed - wales 0 to 3"));
        assert!(
            exported
                .instructions_text
                .contains("Course 2 - Front bed: k4")
        );
        assert!(
            exported
                .instructions_text
                .contains("Course 2 - Back bed: p4")
        );
        assert!(!exported.instructions_text.contains("(WS)"));

        let objects = pdf_objects(&exported.pdf_bytes);
        let page_count = objects
            .values()
            .filter(|(dict, _)| {
                let text = String::from_utf8_lossy(dict);
                text.contains("/Type /Page") && !text.contains("/Type /Pages")
            })
            .count();
        assert_eq!(page_count, 2);
        let streams: Vec<_> = objects
            .values()
            .filter_map(|(_, stream)| stream.as_ref())
            .map(|stream| String::from_utf8_lossy(stream))
            .collect();
        for bed in ["Front bed", "Back bed"] {
            assert_eq!(
                streams.iter().filter(|stream| stream.contains(bed)).count(),
                1
            );
        }
        assert!(
            check_hand_knit_svg(&exported.svg_text, "two-beds.svg")
                .iter()
                .all(|check| check.ok)
        );
        assert!(
            check_hand_knit_pdf(&exported.pdf_bytes, "two-beds.pdf")
                .iter()
                .all(|check| check.ok)
        );

        // Source storage order is not chart order; neither bed wins by appearing last.
        doc.stitches.reverse();
        let reordered = export_hand_knit_document(&doc, "2026-11-27T08:00").unwrap();
        assert_eq!(exported.svg_text, reordered.svg_text);
        assert_eq!(exported.pdf_bytes, reordered.pdf_bytes);
        assert_eq!(exported.instructions_text, reordered.instructions_text);
    }

    #[test]
    fn chart_refuses_same_bed_collisions_and_bounds_all_rendered_panels() {
        let profile = MachineProfile::kniterate_7e();
        let doc = draft_calibration_swatch(&profile, "test-yarn", 4, 2, &[6.0]).unwrap();
        let mut duplicate = doc.clone();
        let mut other = duplicate.stitches[0].clone();
        other.id = duplicate.stitches.len() as u32;
        duplicate.stitches.push(other);
        duplicate.validate().unwrap();
        assert_eq!(
            export_hand_knit_document(&duplicate, "2026-11-27T08:00")
                .unwrap_err()
                .code,
            RefusalCode::InvalidDocument
        );

        let mut large = doc;
        large.stitches.truncate(2);
        large.stitches[1].course = 1000;
        large.stitches[1].wale = 299;
        large.stitches[1].bed = Bed::Back;
        large.validate().unwrap();
        assert_eq!(
            export_hand_knit_document(&large, "2026-11-27T08:00")
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );
    }

    #[test]
    fn full_tier_a_and_tier_b_knitting_export_suite_passes() {
        // 1. Kniterate 7E verified swatch handoff; executable KCode stays unavailable.
        let kniterate = MachineProfile::kniterate_7e();
        let kit = export_calibration_swatch_kit(
            &kniterate,
            "wool-200tex",
            "mat-sha-test",
            24,
            10,
            &[6.0, 8.0],
        )
        .unwrap();
        assert!(!kit.knitout.cert.hardware_execution_authorized);
        assert!(!kit.qualification_template.was_washed_and_blocked);

        let compiled =
            compile_knit_doc(&kit.doc, &kniterate, None, Some("mat-sha-test"), None).unwrap();
        assert!(kit.knitout.knitout_text.starts_with(";!knitout-2"));
        assert_eq!(
            export_kniterate_kc(&compiled, &kniterate).unwrap_err().code,
            RefusalCode::UnqualifiedProcess
        );

        // 2. Brother KH-930 AYAB 1-bit PNG + synchronized checklist
        let kh930 = MachineProfile::brother_kh930_ayab();
        let ayab_doc = draft_calibration_swatch(&kh930, "wool-150tex", 20, 8, &[5.0]).unwrap();
        let ayab_compiled = compile_knit_doc(&ayab_doc, &kh930, None, None, None).unwrap();
        let ayab = export_brother_ayab(&ayab_doc, &kh930, &ayab_compiled.cert).unwrap();
        assert!(ayab.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
        assert_eq!(ayab.png_bytes[24], 1); // 1-bit depth
        assert_eq!(ayab.png_bytes[25], 3); // indexed palette
        assert!(!ayab.checklist.automated_carriage_control);

        // 3. Human hand-knit pure-vector CYC SVG & PDF + dedicated acceptance checks
        let hand = export_hand_knit_document(&kit.doc, "2026-11-27T08:00").unwrap();
        let svg_checks = check_hand_knit_svg(&hand.svg_text, "swatch.svg");
        assert!(
            svg_checks.iter().all(|c| c.ok),
            "SVG checks: {svg_checks:?}"
        );
        let pdf_checks = check_hand_knit_pdf(&hand.pdf_bytes, "swatch.pdf");
        assert!(
            pdf_checks.iter().all(|c| c.ok),
            "PDF checks: {pdf_checks:?}"
        );

        // 4. Tier-B Shima Seiki direct proprietary binary refused fail-closed
        let shima = MachineProfile::shima_seiki_12e_handoff();
        let err = refuse_proprietary_binary_target(&shima, ".dat").unwrap_err();
        assert_eq!(err.code, RefusalCode::OpaqueProprietaryTarget);
    }

    #[test]
    fn knit_exporters_fail_closed_without_silent_clamping_or_false_svg_negatives() {
        let kniterate = MachineProfile::kniterate_7e();
        let mut doc = draft_calibration_swatch(&kniterate, "wool-200tex", 12, 4, &[6.0]).unwrap();
        // Pattern whose name contains "transform" must NOT trigger SVG no_transform false negative!
        doc.name = "transformer_vest".to_string();
        let hand = export_hand_knit_document(&doc, "2026-11-27T08:00").unwrap();
        assert!(
            check_hand_knit_svg(&hand.svg_text, "vest.svg")
                .iter()
                .all(|c| c.ok)
        );
        // Actual SVG transform attribute MUST still fail!
        let bad_svg = hand.svg_text.replace(
            r#"<g id="cyc-legend">"#,
            r#"<g id="cyc-legend" transform="scale(2)">"#,
        );
        assert!(
            !check_hand_knit_svg(&bad_svg, "bad.svg")
                .iter()
                .all(|c| c.ok)
        );
        // PDF with only /Type /Pages and no page/stream must fail check_hand_knit_pdf
        let fake_pdf =
            b"%PDF-1.4\n1 0 obj\n<< /Type /Pages /BaseFont /Helvetica >>\nendobj\n%%EOF\n";
        assert!(
            !check_hand_knit_pdf(fake_pdf, "fake.pdf")
                .iter()
                .all(|c| c.ok)
        );

        // Extreme wale span refused cleanly with BudgetExceeded instead of overflowing/OOMing
        let mut huge_doc = doc.clone();
        huge_doc.stitches[0].wale = i32::MIN + 1;
        huge_doc.stitches[1].wale = i32::MAX - 1;
        assert_eq!(
            export_hand_knit_document(&huge_doc, "2026-11-27T08:00")
                .unwrap_err()
                .code,
            RefusalCode::BudgetExceeded
        );

        // Kniterate validates the program digest and machine before its backend refusal.
        let compiled = compile_knit_doc(&doc, &kniterate, None, None, None).unwrap();
        let mut tampered = compiled.clone();
        tampered
            .program
            .ops
            .push(ftx_pattern::knit::KnitoutOp::Comment {
                text: "tamper".to_string(),
            });
        assert_eq!(
            export_kniterate_kc(&tampered, &kniterate).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );

        let mut bad_prof = kniterate.clone();
        bad_prof.min_carrier_spacing = 1;
        assert_eq!(
            export_kniterate_kc(&compiled, &bad_prof).unwrap_err().code,
            RefusalCode::MachineStateInfeasible
        );

        // Brother AYAB refuses mismatched cert machine hash
        let kh930 = MachineProfile::brother_kh930_ayab();
        assert_eq!(
            export_brother_ayab(&doc, &kh930, &compiled.cert)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
    }
}
