//! Human hand-knitting pure-vector CYC chart SVG & PDF exporter (`FTX-K07`) with dedicated
//! acceptance checks (`check_hand_knit_svg`, `check_hand_knit_pdf`).
//!
//! Every Craft Yarn Council (CYC) stitch symbol is drawn as explicit 2D vector geometry (lines,
//! polylines, filled circles) with ZERO Unicode symbol glyphs so output renders crisply in
//! WinAnsi `/Helvetica` PDF viewers and standard SVG renderers without `transform` attributes.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, Vec2};
use ftx_pattern::knit::{KnitDoc, LoopKind};

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

    let mut min_w = i32::MAX;
    let mut max_w = i32::MIN;
    let mut max_c = 0u32;
    let mut grid: BTreeMap<(u32, i32), LoopKind> = BTreeMap::new();
    for st in &doc.stitches {
        min_w = min_w.min(st.wale);
        max_w = max_w.max(st.wale);
        max_c = max_c.max(st.course);
        grid.insert((st.course, st.wale), st.kind);
    }
    let cols = (max_w - min_w + 1) as usize;
    let rows = (max_c + 1) as usize;

    // Layout geometry in mm (y down on page)
    let margin = 15.0;
    let header_h = 28.0;
    let cell = (160.0 / (cols.max(1) as f64)).clamp(3.0, 6.0);
    let chart_w = cols as f64 * cell;
    let chart_h = rows as f64 * cell;
    let cal_block_w = 112.0; // holds 100 mm calibration square + legend
    let page_w = (margin * 2.0 + chart_w.max(cal_block_w) + 18.0).max(210.0);
    let page_h = (margin * 2.0 + header_h + chart_h + 125.0).max(297.0);
    let chart_x = margin + 10.0;
    let chart_y = margin + header_h;

    // 1. Build pure-vector SVG (NO transform attributes!)
    let mut svg = String::new();
    let _ = writeln!(
        svg,
        r#"<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="{SVG_NAMESPACE}" width="{w}mm" height="{h}mm" viewBox="0 0 {w} {h}">"#,
        w = fnum(page_w, 2),
        h = fnum(page_h, 2)
    );
    let _ = writeln!(
        svg,
        r#"<rect x="0" y="0" width="{w}" height="{h}" fill="white"/>"#,
        w = fnum(page_w, 2),
        h = fnum(page_h, 2)
    );
    let title = ascii_text(&format!("FrankenTextiles Hand-Knit Chart: {}", doc.name));
    let gauge_line = ascii_text(&format!(
        "Gauge: {:.1} wales x {:.1} courses per 100mm | Stitches: {} x {} | Wearer: {:?}",
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
    for r in 0..rows {
        let course = (rows - 1 - r) as u32;
        let y0 = chart_y + r as f64 * cell;
        for c in 0..cols {
            let wale = min_w + c as i32;
            let x0 = chart_x + c as f64 * cell;
            if let Some(&kind) = grid.get(&(course, wale)) {
                let _ = write!(
                    svg,
                    r##"<rect x="{x}" y="{y}" width="{s}" height="{s}" fill="none" stroke="#444444" stroke-width="0.22"/>"##,
                    x = fnum(x0, 2),
                    y = fnum(y0, 2),
                    s = fnum(cell, 2)
                );
                emit_cyc_symbol_svg(&mut svg, kind, x0, y0, cell);
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
        if row_num == 1 || row_num % 5 == 0 {
            let _ = write!(
                svg,
                r#"<text x="{x}" y="{y}" font-family="Helvetica, sans-serif" font-size="2.5">{row_num}</text>"#,
                x = fnum(chart_x + chart_w + 2.0, 2),
                y = fnum(y0 + cell * 0.72, 2)
            );
        }
    }
    svg.push_str("</g>\n");

    // 100 mm x 100 mm physical print calibration square + CYC vector legend
    let cal_x = margin;
    let cal_y = chart_y + chart_h + 12.0;
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
        vec!["Hand-Knit Chart".to_string(), "Calibration Grid".to_string()],
        &[],
        title.clone(),
    );
    let mut cv = pdf.canvas();
    cv.fill_gray(0.0);
    cv.stroke_gray(0.15);
    cv.text(margin, margin + 5.0, &title, 5.0, TextAnchor::Start, true, None);
    cv.text(margin, margin + 12.0, &gauge_line, 3.0, TextAnchor::Start, false, None);

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
            if let Some(&kind) = grid.get(&(course, wale)) {
                cv.stroke_gray(0.25);
                cv.path(&box_pts, true);
                cv.raw("S");
                emit_cyc_symbol_pdf(&mut cv, kind, x0, y0, cell);
            } else {
                cv.fill_gray(0.85);
                cv.path(&box_pts, true);
                cv.raw("f");
            }
        }
        let row_num = course + 1;
        if row_num == 1 || row_num % 5 == 0 {
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
    let pdf_bytes = pdf.write(created);

    // 3. Build deterministic ASCII written row instructions
    let mut instr = String::new();
    let _ = writeln!(instr, "# {title}");
    let _ = writeln!(instr, "{gauge_line}\n");
    for course in 0..=max_c {
        let side = if course % 2 == 0 { "RS" } else { "WS" };
        let mut counts: Vec<(LoopKind, usize)> = Vec::new();
        for wale in min_w..=max_w {
            if let Some(&kind) = grid.get(&(course, wale)) {
                if let Some(last) = counts.last_mut() {
                    if last.0 == kind {
                        last.1 += 1;
                        continue;
                    }
                }
                counts.push((kind, 1));
            }
        }
        let tokens: Vec<String> = counts
            .into_iter()
            .map(|(k, n)| format!("{}{n}", cyc_abbrev(k)))
            .collect();
        let _ = writeln!(instr, "Row {} ({side}): {}", course + 1, tokens.join(", "));
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

/// Dedicated acceptance check for human hand-knit CYC vector SVG outputs (`FTX-K07`).
#[must_use]
pub fn check_hand_knit_svg(text: &str, file: &str) -> Vec<CheckItem> {
    let parsed = parse(text);
    let root = parsed.as_ref();
    let attr = |k: &str| root.and_then(|r| r.get(k)).unwrap_or("").to_string();
    let (w, h, vb) = (attr("width"), attr("height"), attr("viewBox"));
    let mm_ok = root.is_some() && w.ends_with("mm") && h.ends_with("mm") && !vb.is_empty();
    vec![
        CheckItem {
            check: "hand_knit_svg.no_transform".to_string(),
            file: file.to_string(),
            ok: !text.contains("transform"),
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
    let has_page = objs
        .values()
        .any(|(dict, _)| String::from_utf8_lossy(dict).contains("/Type /Page"));
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
            ok: has_page && has_font,
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
    use ftx_pattern::knit::{MachineProfile, compile_knit_doc, draft_calibration_swatch};

    #[test]
    fn full_tier_a_and_tier_b_knitting_export_suite_passes() {
        // 1. Kniterate 7E swatch kit + .kc dialect
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

        let compiled = compile_knit_doc(&kit.doc, &kniterate, None, Some("mat-sha-test"), None).unwrap();
        let kc = export_kniterate_kc(&compiled, &kniterate).unwrap();
        assert!(kc.kc_text.contains("HEADER MACHINE=KNITERATE_7E NEEDLES=252"));
        assert!(kc.kc_text.contains("GRIPPER_IN 1"));
        assert!(!kc.kc_text.contains("inhook"));

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
        assert!(svg_checks.iter().all(|c| c.ok), "SVG checks: {svg_checks:?}");
        let pdf_checks = check_hand_knit_pdf(&hand.pdf_bytes, "swatch.pdf");
        assert!(pdf_checks.iter().all(|c| c.ok), "PDF checks: {pdf_checks:?}");

        // 4. Tier-B Shima Seiki direct proprietary binary refused fail-closed
        let shima = MachineProfile::shima_seiki_12e_handoff();
        let err = refuse_proprietary_binary_target(&shima, ".dat").unwrap_err();
        assert_eq!(err.code, RefusalCode::OpaqueProprietaryTarget);
    }
}
