//! The hand-written PDF writer and the PDF targets (`docs/spec/03` §7.2–7.6): tiled Letter and A4 home prints,
//! copy-shop A0 and 36 × 48 in sheets, and the projector file in light and dark. Every page draws in
//! millimetres with the origin top left and y down; layers are optional content groups named as in the
//! reference so pattern projectors merge them across files.

use std::collections::BTreeSet;

use ftx_core::{Refusal, RefusalCode, Vec2, np, py};
use serde_json::{Value, json};

use crate::ctx::{Ctx, primary_instance};
use crate::geom::{Affine, CAP, Geom, Source, grain_marks, label_texts};
use crate::layout::{Layout, shelf_layout};
use crate::text::{ascii_text, column_letters, fnum, page_list, pdf_string, text_width, wrap};

/// PDF points per millimetre.
pub const PT_PER_MM: f64 = 72.0 / 25.4;
/// Millimetres per inch.
pub const INCH: f64 = 25.4;
/// The layers every PDF has after its size layer.
pub const OCG_COMMON: [&str; 5] = [
    "Notches",
    "Seam lines",
    "Grainlines & labels",
    "Placement marks",
    "Soutache placement",
];
const GRID_OCG: &str = "Calibration grid (10 cm / 1 in)";

/// Where text sits on its `x`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextAnchor {
    /// Starts at `x`.
    Start,
    /// Centred on `x`.
    Middle,
    /// Ends at `x`.
    End,
}

/// A content stream under construction, one operator group per line.
#[derive(Clone, Debug, Default)]
pub struct Canvas {
    /// The lines.
    pub ops: Vec<String>,
    ocgs: Vec<String>,
}

impl Canvas {
    fn raw(&mut self, s: impl Into<String>) {
        self.ops.push(s.into());
    }
    fn save(&mut self) {
        self.raw("q");
    }
    fn restore(&mut self) {
        self.raw("Q");
    }
    fn width(&mut self, w: f64) {
        self.raw(format!("{} w", fnum(w, 4)));
    }
    fn dash(&mut self, arr: &[f64]) {
        let a: Vec<String> = arr.iter().map(|&v| fnum(v, 3)).collect();
        self.raw(format!("[{}] 0 d", a.join(" ")));
    }
    fn stroke_gray(&mut self, g: f64) {
        self.raw(format!("{} G", fnum(g, 3)));
    }
    fn fill_gray(&mut self, g: f64) {
        self.raw(format!("{} g", fnum(g, 3)));
    }
    fn stroke_rgb(&mut self, c: [f64; 3]) {
        self.raw(format!(
            "{} {} {} RG",
            fnum(c[0], 3),
            fnum(c[1], 3),
            fnum(c[2], 3)
        ));
    }
    fn fill_rgb(&mut self, c: [f64; 3]) {
        self.raw(format!(
            "{} {} {} rg",
            fnum(c[0], 3),
            fnum(c[1], 3),
            fnum(c[2], 3)
        ));
    }
    fn path(&mut self, pts: &[Vec2], closed: bool) {
        let mut seg = vec![format!("{} {} m", fnum(pts[0].x, 3), fnum(pts[0].y, 3))];
        seg.extend(
            pts[1..]
                .iter()
                .map(|p| format!("{} {} l", fnum(p.x, 3), fnum(p.y, 3))),
        );
        if closed {
            seg.push("h".into());
        }
        self.raw(seg.join(" "));
    }
    fn polyline(&mut self, pts: &[Vec2], closed: bool) {
        self.path(pts, closed);
        self.raw("S");
    }
    fn rect(&mut self, x: f64, y: f64, w: f64, h: f64, op: &str) {
        self.raw(format!(
            "{} {} {} {} re {op}",
            fnum(x, 3),
            fnum(y, 3),
            fnum(w, 3),
            fnum(h, 3)
        ));
    }
    fn clip_rect(&mut self, x: f64, y: f64, w: f64, h: f64) {
        self.rect(x, y, w, h, "W n");
    }
    fn circle(&mut self, cx: f64, cy: f64, r: f64) {
        let k = 0.5523 * r;
        let f = |v: f64| fnum(v, 3);
        self.raw(format!(
            "{} {} m {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c {} {} {} {} {} {} c S",
            f(cx + r),
            f(cy),
            f(cx + r),
            f(cy + k),
            f(cx + k),
            f(cy + r),
            f(cx),
            f(cy + r),
            f(cx - k),
            f(cy + r),
            f(cx - r),
            f(cy + k),
            f(cx - r),
            f(cy),
            f(cx - r),
            f(cy - k),
            f(cx - k),
            f(cy - r),
            f(cx),
            f(cy - r),
            f(cx + k),
            f(cy - r),
            f(cx + r),
            f(cy - k),
            f(cx + r),
            f(cy)
        ));
    }
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        x: f64,
        y: f64,
        s: &str,
        size: f64,
        anchor: TextAnchor,
        bold: bool,
        halo: Option<f64>,
    ) {
        let s = ascii_text(s);
        let w = text_width(&s, size, bold);
        let x0 = x - match anchor {
            TextAnchor::Middle => w / 2.0,
            TextAnchor::End => w,
            TextAnchor::Start => 0.0,
        };
        let esc = pdf_string(&s);
        let font = if bold { "/F2" } else { "/F1" };
        if let Some(g) = halo {
            self.raw(format!(
                "q {} G {} w 1 j BT {font} {} Tf 1 Tr 1 0 0 -1 {} {} Tm {esc} Tj ET Q",
                fnum(g, 3),
                fnum(size * 0.12, 3),
                fnum(size, 3),
                fnum(x0, 3),
                fnum(y, 3)
            ));
        }
        self.raw(format!(
            "BT {font} {} Tf 0 Tr 1 0 0 -1 {} {} Tm {esc} Tj ET",
            fnum(size, 3),
            fnum(x0, 3),
            fnum(y, 3)
        ));
    }
    fn begin_oc(&mut self, name: &str) {
        let i = self.ocgs.iter().position(|n| n == name).unwrap_or(0);
        self.raw(format!("/OC /oc{i} BDC"));
    }
    fn end_oc(&mut self) {
        self.raw("EMC");
    }
    fn tag(&mut self, name: &str) {
        self.raw(format!("/{name} BMC"));
    }
    fn end_tag(&mut self) {
        self.raw("EMC");
    }
}

/// A PDF document: its layers (those in `off` start hidden), title and pages.
#[derive(Clone, Debug)]
pub struct PdfDoc {
    ocgs: Vec<String>,
    off: Vec<String>,
    title: String,
    pages: Vec<(f64, f64, Canvas, String)>,
}

impl PdfDoc {
    fn new(ocgs: Vec<String>, off: &[&str], title: String) -> Self {
        Self {
            ocgs,
            off: off.iter().map(|s| (*s).to_string()).collect(),
            title,
            pages: Vec::new(),
        }
    }

    fn canvas(&self) -> Canvas {
        Canvas {
            ops: Vec::new(),
            ocgs: self.ocgs.clone(),
        }
    }

    fn add_page(&mut self, w: f64, h: f64, cv: Canvas, extra: String) {
        self.pages.push((w, h, cv, extra));
    }

    /// The file: header, objects (catalog, pages, the two fonts, the layers, per page its content stream and
    /// page dictionary, the info dictionary), cross-reference table and trailer. `created` (the pattern's
    /// `YYYY-MM-DDTHH:MM`) gives the creation date, so the file is reproducible.
    #[must_use]
    pub fn write(&self, created: &str) -> Vec<u8> {
        let mut objs: Vec<Option<Vec<u8>>> = vec![None];
        let add = |objs: &mut Vec<Option<Vec<u8>>>, b: Vec<u8>| {
            objs.push(Some(b));
            objs.len() - 1
        };
        objs.push(None);
        let cat = objs.len() - 1;
        objs.push(None);
        let pages = objs.len() - 1;
        let f1 = add(
            &mut objs,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        );
        let f2 = add(
            &mut objs,
            b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold /Encoding /WinAnsiEncoding >>"
                .to_vec(),
        );
        let oc_refs: Vec<usize> = self
            .ocgs
            .iter()
            .map(|n| {
                add(
                    &mut objs,
                    format!(
                        "<< /Type /OCG /Name {} /Intent [/View /Design] >>",
                        pdf_string(n)
                    )
                    .into_bytes(),
                )
            })
            .collect();
        let props: Vec<String> = oc_refs
            .iter()
            .enumerate()
            .map(|(i, r)| format!("/oc{i} {r} 0 R"))
            .collect();
        let props = props.join(" ");
        let mut kids = Vec::new();
        for (w, h, cv, extra) in &self.pages {
            let (wp, hp) = (w * PT_PER_MM, h * PT_PER_MM);
            let body = format!(
                "q {:.10} 0 0 {:.10} 0 {hp:.4} cm\n1 j 1 J 10 M\n{}\nQ\n",
                PT_PER_MM,
                -PT_PER_MM,
                cv.ops.join("\n")
            );
            let data = miniz_oxide::deflate::compress_to_vec_zlib(body.as_bytes(), 9);
            let mut stream = format!(
                "<< /Length {} /Filter /FlateDecode >>\nstream\n",
                data.len()
            )
            .into_bytes();
            stream.extend_from_slice(&data);
            stream.extend_from_slice(b"\nendstream");
            let content = add(&mut objs, stream);
            kids.push(add(
                &mut objs,
                format!(
                    "<< /Type /Page /Parent {pages} 0 R /MediaBox [0 0 {wp:.4} {hp:.4}] /Resources << /Font << \
                     /F1 {f1} 0 R /F2 {f2} 0 R >> /Properties << {props} >> >> /Contents {content} 0 R {extra}>>"
                )
                .into_bytes(),
            ));
        }
        let kid_refs: Vec<String> = kids.iter().map(|k| format!("{k} 0 R")).collect();
        objs[pages] = Some(
            format!(
                "<< /Type /Pages /Kids [{}] /Count {} >>",
                kid_refs.join(" "),
                kids.len()
            )
            .into_bytes(),
        );
        let refs = |pick: &dyn Fn(&str) -> bool| -> String {
            self.ocgs
                .iter()
                .zip(&oc_refs)
                .filter(|(n, _)| pick(n))
                .map(|(_, r)| format!("{r} 0 R"))
                .collect::<Vec<_>>()
                .join(" ")
        };
        let on = refs(&|n| !self.off.iter().any(|o| o == n));
        let offs = refs(&|n| self.off.iter().any(|o| o == n));
        let all = refs(&|_| true);
        objs[cat] = Some(
            format!(
                "<< /Type /Catalog /Pages {pages} 0 R /PageMode /UseOC /OCProperties << /OCGs [{all}] /D << \
                 /Name (Layers) /Order [{all}] /ON [{on}] /OFF [{offs}] /RBGroups [] >> >> /ViewerPreferences \
                 << /PrintScaling /None /PickTrayByPDFSize true >> >>"
            )
            .into_bytes(),
        );
        let digits: String = created.chars().filter(char::is_ascii_digit).collect();
        let date: String = format!("{digits:0<14}").chars().take(14).collect();
        let info = add(
            &mut objs,
            format!(
                "<< /Title {} /Producer (FrankenTextiles ftx-export) /CreationDate (D:{date}) >>",
                pdf_string(&self.title)
            )
            .into_bytes(),
        );
        let mut buf = b"%PDF-1.6\n%\xe2\xe3\xcf\xd3\n".to_vec();
        let mut offsets = Vec::new();
        for (i, o) in objs.iter().enumerate().skip(1) {
            offsets.push(buf.len());
            buf.extend_from_slice(format!("{i} 0 obj\n").as_bytes());
            buf.extend_from_slice(o.as_deref().unwrap_or_default());
            buf.extend_from_slice(b"\nendobj\n");
        }
        let xref = buf.len();
        buf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len()).as_bytes());
        for o in offsets {
            buf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
        }
        buf.extend_from_slice(
            format!(
                "trailer\n<< /Size {} /Root {cat} 0 R /Info {info} 0 R >>\nstartxref\n{xref}\n%%EOF\n",
                objs.len()
            )
            .as_bytes(),
        );
        buf
    }
}

/// Line widths, dashes, text and ink of one target.
#[derive(Clone, Copy, Debug)]
struct PdfStyle {
    cut: f64,
    sew: f64,
    notch: f64,
    thin: f64,
    dash: [f64; 2],
    text_scale: f64,
    min_text: f64,
    arrow: f64,
    ink: Option<[f64; 3]>,
    cut_ink: Option<[f64; 3]>,
    halo: f64,
}

impl PdfStyle {
    fn tiles(k: f64) -> Self {
        Self {
            cut: 0.3 * k,
            sew: 0.2 * k,
            notch: 0.3 * k,
            thin: 0.2 * k,
            dash: [3.0 * k, 1.5 * k],
            text_scale: 1.0,
            min_text: 0.0,
            arrow: 6.0 * k,
            ink: None,
            cut_ink: None,
            halo: 1.0,
        }
    }
}

fn ink(cv: &mut Canvas, rgb: Option<[f64; 3]>) {
    match rgb {
        None => {
            cv.stroke_gray(0.0);
            cv.fill_gray(0.0);
        }
        Some(c) => {
            cv.stroke_rgb(c);
            cv.fill_rgb(c);
        }
    }
}

fn draw_piece(cv: &mut Canvas, g: &Geom, size_ocg: &str, st: &PdfStyle) {
    cv.begin_oc(size_ocg);
    ink(cv, st.cut_ink.or(st.ink));
    cv.width(st.cut);
    cv.dash(&[]);
    cv.tag("FPCut");
    cv.polyline(&g.cut, true);
    cv.end_tag();
    for (p, c) in &g.internal_cut {
        cv.polyline(p, *c);
    }
    ink(cv, st.ink);
    cv.begin_oc("Seam lines");
    cv.width(st.sew);
    cv.dash(&st.dash);
    cv.polyline(&g.sew, true);
    cv.dash(&[]);
    cv.end_oc();
    cv.begin_oc("Notches");
    cv.width(st.notch);
    for n in &g.notches {
        for mk in &n.marks {
            cv.polyline(mk, false);
        }
    }
    cv.end_oc();
    cv.begin_oc("Placement marks");
    cv.width(st.thin);
    let ts = st.text_scale;
    for (p, s) in &g.fold_lines {
        if s == "dashdot" {
            cv.dash(&[6.0 * ts, 1.5 * ts, 1.0 * ts, 1.5 * ts]);
        }
        cv.polyline(p, false);
        cv.dash(&[]);
    }
    cv.dash(&[2.0 * ts, 1.0 * ts]);
    for (p, c) in &g.internal_draw {
        cv.polyline(p, *c);
    }
    cv.dash(&[]);
    for &(p, d) in &g.drills {
        cv.circle(p.x, p.y, d / 2.0);
        cv.polyline(&[Vec2::new(p.x - d, p.y), Vec2::new(p.x + d, p.y)], false);
        cv.polyline(&[Vec2::new(p.x, p.y - d), Vec2::new(p.x, p.y + d)], false);
    }
    for lb in g
        .labels
        .iter()
        .filter(|l| l.kind == "fold" || l.kind == "internal")
    {
        for (x, y, t, s, bold) in label_texts(lb, true, st.min_text, st.text_scale) {
            cv.text(x, y, &t, s, TextAnchor::Middle, bold, Some(st.halo));
        }
    }
    cv.end_oc();
    cv.begin_oc("Grainlines & labels");
    cv.width(st.thin * 1.3);
    for mk in grain_marks(g, st.arrow) {
        cv.polyline(&mk, false);
    }
    for lb in g.labels.iter().filter(|l| l.kind == "label") {
        for (x, y, t, s, bold) in label_texts(lb, true, st.min_text, st.text_scale) {
            cv.text(x, y, &t, s, TextAnchor::Middle, bold, Some(st.halo));
        }
    }
    cv.end_oc();
    if !g.decor.is_empty() {
        cv.begin_oc("Soutache placement");
        for d in &g.decor {
            cv.width(d.width);
            cv.stroke_gray(if st.ink.is_none() { 0.7 } else { 0.45 });
            cv.polyline(&d.pts, d.closed);
            ink(cv, st.ink);
            cv.width(st.thin);
            cv.polyline(&d.pts, d.closed);
        }
        cv.end_oc();
    }
    cv.end_oc();
}

/// The 4 in and 10 cm squares with ruler ticks along their bottom and left edges.
fn draw_test_block(cv: &mut Canvas, x: f64, y: f64, vertical: bool, tsize: f64) {
    let gap = 8.0;
    cv.stroke_gray(0.0);
    cv.fill_gray(0.0);
    cv.dash(&[]);
    let squares = [
        ("FPTest4in", 4.0 * INCH, "4 in x 4 in", "101.6 mm"),
        ("FPTest10cm", 100.0, "10 cm x 10 cm", "100 mm"),
    ];
    let pos = [
        (x, y),
        if vertical {
            (x, y + 4.0 * INCH + gap)
        } else {
            (x + 4.0 * INCH + gap, y)
        },
    ];
    for ((tag, s, l1, l2), (px, py)) in squares.into_iter().zip(pos) {
        cv.width(0.3);
        cv.tag(tag);
        cv.rect(px, py, s, s, "S");
        cv.end_tag();
        cv.width(0.15);
        let ticks: Vec<(f64, f64)> = if tag == "FPTest4in" {
            (1..32)
                .map(|k| {
                    let len = if k % 8 == 0 {
                        6.0
                    } else if k % 4 == 0 {
                        4.0
                    } else {
                        2.0
                    };
                    (f64::from(k) * INCH / 8.0, len)
                })
                .collect()
        } else {
            (1..100)
                .map(|k| {
                    let len = if k % 10 == 0 {
                        6.0
                    } else if k % 5 == 0 {
                        4.0
                    } else {
                        2.0
                    };
                    (f64::from(k), len)
                })
                .collect()
        };
        for (d, ln) in ticks {
            cv.polyline(
                &[Vec2::new(px + d, py + s), Vec2::new(px + d, py + s - ln)],
                false,
            );
            cv.polyline(
                &[Vec2::new(px, py + s - d), Vec2::new(px + ln, py + s - d)],
                false,
            );
        }
        let (cx, cy) = (px + s / 2.0, py + s / 2.0);
        cv.text(cx, cy - 1.0, l1, tsize, TextAnchor::Middle, true, None);
        cv.text(
            cx,
            cy + tsize * 1.5,
            l2,
            tsize * 0.8,
            TextAnchor::Middle,
            false,
            None,
        );
        cv.text(
            cx,
            cy + tsize * 3.0,
            "ticks: X along bottom, Y along left",
            tsize * 0.6,
            TextAnchor::Middle,
            false,
            None,
        );
    }
}

fn draw_scale_bars(cv: &mut Canvas, x: f64, y: f64, tsize: f64) {
    cv.width(0.3);
    cv.stroke_gray(0.0);
    cv.fill_gray(0.0);
    cv.polyline(&[Vec2::new(x, y), Vec2::new(x + 300.0, y)], false);
    for k in (0..=300).step_by(10) {
        let kf = f64::from(k);
        cv.polyline(
            &[
                Vec2::new(x + kf, y),
                Vec2::new(x + kf, y - if k % 50 == 0 { 6.0 } else { 3.0 }),
            ],
            false,
        );
        if k % 50 == 0 {
            cv.text(
                x + kf,
                y - 8.0,
                &(k / 10).to_string(),
                tsize * 0.7,
                TextAnchor::Middle,
                false,
                None,
            );
        }
    }
    cv.text(
        x + 305.0,
        y,
        "cm",
        tsize * 0.7,
        TextAnchor::Start,
        false,
        None,
    );
    let yi = y + 22.0;
    cv.polyline(&[Vec2::new(x, yi), Vec2::new(x + 12.0 * INCH, yi)], false);
    for k in 0..49 {
        let d = f64::from(k) * INCH / 4.0;
        cv.polyline(
            &[
                Vec2::new(x + d, yi),
                Vec2::new(x + d, yi - if k % 4 == 0 { 6.0 } else { 3.0 }),
            ],
            false,
        );
        if k % 4 == 0 {
            cv.text(
                x + d,
                yi - 8.0,
                &(k / 4).to_string(),
                tsize * 0.7,
                TextAnchor::Middle,
                false,
                None,
            );
        }
    }
    cv.text(
        x + 12.0 * INCH + 5.0,
        yi,
        "in",
        tsize * 0.7,
        TextAnchor::Start,
        false,
        None,
    );
}

/// A sheet: page size and the printable frame (mm).
#[derive(Clone, Debug, PartialEq)]
pub struct Paper {
    /// `LETTER`, `A4`, `A0`, `36x48in`.
    pub name: String,
    /// Page width.
    pub w: f64,
    /// Page height.
    pub h: f64,
    /// Frame left.
    pub fx: f64,
    /// Frame top.
    pub fy: f64,
    /// Frame width.
    pub fw: f64,
    /// Frame height.
    pub fh: f64,
    /// A copy-shop sheet (information band below the frame).
    pub band: bool,
}

impl Paper {
    /// US Letter, half-inch insets.
    #[must_use]
    pub fn letter() -> Self {
        Self {
            name: "LETTER".into(),
            w: 215.9,
            h: 279.4,
            fx: 12.7,
            fy: 12.7,
            fw: 190.5,
            fh: 254.0,
            band: false,
        }
    }

    /// ISO A4, 10 mm insets.
    #[must_use]
    pub fn a4() -> Self {
        Self {
            name: "A4".into(),
            w: 210.0,
            h: 297.0,
            fx: 10.0,
            fy: 10.0,
            fw: 190.0,
            fh: 277.0,
            band: false,
        }
    }

    fn copyshop(name: &str, w: f64, h: f64) -> Self {
        Self {
            name: name.into(),
            w,
            h,
            fx: 10.0,
            fy: 10.0,
            fw: w - 20.0,
            fh: h - 10.0 - 135.0,
            band: true,
        }
    }
}

/// One tile of the master layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tile {
    /// Column.
    pub c: usize,
    /// Row.
    pub r: usize,
    /// `A1`, `B1`, ...
    pub name: String,
    /// Nothing of a cut line on it.
    pub blank: bool,
}

fn cross(o: Vec2, a: Vec2, b: Vec2) -> f64 {
    (a.x - o.x) * (b.y - o.y) - (a.y - o.y) * (b.x - o.x)
}

fn segments_cross(a: Vec2, b: Vec2, c: Vec2, d: Vec2) -> bool {
    let (d1, d2, d3, d4) = (
        cross(c, d, a),
        cross(c, d, b),
        cross(a, b, c),
        cross(a, b, d),
    );
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0))
        && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0))
    {
        return true;
    }
    let on = |p: Vec2, q: Vec2, r: Vec2| {
        r.x >= p.x.min(q.x) && r.x <= p.x.max(q.x) && r.y >= p.y.min(q.y) && r.y <= p.y.max(q.y)
    };
    (d1 == 0.0 && on(c, d, a))
        || (d2 == 0.0 && on(c, d, b))
        || (d3 == 0.0 && on(a, b, c))
        || (d4 == 0.0 && on(a, b, d))
}

fn point_seg_dist(p: Vec2, a: Vec2, b: Vec2) -> f64 {
    let d = b - a;
    let l2 = d.x * d.x + d.y * d.y;
    let t = if l2 > 0.0 {
        (((p.x - a.x) * d.x + (p.y - a.y) * d.y) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p - (a + d * t)).norm()
}

/// Distance between a closed polygon (with its inside) and an axis-aligned rectangle; 0 when they meet.
fn poly_rect_distance(poly: &[Vec2], lo: Vec2, hi: Vec2) -> f64 {
    let inside_rect = |p: Vec2| p.x >= lo.x && p.x <= hi.x && p.y >= lo.y && p.y <= hi.y;
    if poly.iter().any(|&p| inside_rect(p)) {
        return 0.0;
    }
    let corners = [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)];
    if ftx_core::polyline::point_in_polygon(corners[0], poly) {
        return 0.0;
    }
    let n = poly.len();
    let mut best = f64::INFINITY;
    for i in 0..n {
        let (a, b) = (poly[i], poly[(i + 1) % n]);
        for j in 0..4 {
            let (c, d) = (corners[j], corners[(j + 1) % 4]);
            if segments_cross(a, b, c, d) {
                return 0.0;
            }
            best = best
                .min(point_seg_dist(a, c, d))
                .min(point_seg_dist(c, a, b));
        }
    }
    best
}

/// The tiles of a layout on a paper's frames, row-major; blank where no cut line reaches the tile (shrunk by
/// 0.01 mm).
#[must_use]
pub fn tile_grid(lay: &Layout, paper: &Paper) -> (usize, usize, Vec<Tile>) {
    let cols = ((lay.width / paper.fw - 1e-9).ceil() as usize).max(1);
    let rows = ((lay.height / paper.fh - 1e-9).ceil() as usize).max(1);
    let mut tiles = Vec::new();
    for r in 0..rows {
        for c in 0..cols {
            let lo = Vec2::new(c as f64 * paper.fw + 0.01, r as f64 * paper.fh + 0.01);
            let hi = Vec2::new(
                (c + 1) as f64 * paper.fw - 0.01,
                (r + 1) as f64 * paper.fh - 0.01,
            );
            let blank = !lay
                .items
                .iter()
                .any(|g| poly_rect_distance(&g.cut, lo, hi) == 0.0);
            tiles.push(Tile {
                c,
                r,
                name: format!("{}{}", column_letters(c), r + 1),
                blank,
            });
        }
    }
    (cols, rows, tiles)
}

#[allow(clippy::too_many_arguments)]
fn tile_map(
    cv: &mut Canvas,
    lay: &Layout,
    paper: &Paper,
    tiles: &[Tile],
    (cols, rows): (usize, usize),
    pages: &[(String, usize)],
    (x, y, bw, bh): (f64, f64, f64, f64),
    ts: f64,
) {
    let s = (bw / (cols as f64 * paper.fw)).min(bh / (rows as f64 * paper.fh));
    cv.save();
    cv.raw(format!("1 0 0 1 {} {} cm", fnum(x, 3), fnum(y, 3)));
    cv.width(0.25);
    cv.dash(&[]);
    for t in tiles {
        let (tx, ty) = (t.c as f64 * paper.fw * s, t.r as f64 * paper.fh * s);
        let (tw, th) = (paper.fw * s, paper.fh * s);
        if t.blank {
            cv.fill_gray(0.88);
            cv.rect(tx, ty, tw, th, "f");
        }
        cv.stroke_gray(0.0);
        cv.rect(tx, ty, tw, th, "S");
    }
    cv.save();
    cv.raw(format!("{} 0 0 {} 0 0 cm", fnum(s, 6), fnum(s, 6)));
    cv.width(0.3 / s);
    cv.stroke_gray(0.35);
    for g in &lay.items {
        cv.polyline(&g.cut, true);
    }
    cv.restore();
    cv.fill_gray(0.0);
    for t in tiles {
        let (tx, ty) = (t.c as f64 * paper.fw * s, t.r as f64 * paper.fh * s);
        let lab = if t.blank {
            format!("{}  blank", t.name)
        } else {
            format!("{}  p{}", t.name, page_of(pages, &t.name))
        };
        cv.text(
            tx + 1.0,
            ty + ts + 0.6,
            &lab,
            ts,
            TextAnchor::Start,
            !t.blank,
            None,
        );
    }
    cv.restore();
}

fn page_of(pages: &[(String, usize)], name: &str) -> usize {
    pages.iter().find(|(n, _)| n == name).map_or(0, |(_, p)| *p)
}

fn half_diamond(
    cv: &mut Canvas,
    mid: Vec2,
    inward: Vec2,
    code: &str,
    ts: f64,
    base: f64,
    height: f64,
) {
    let along = Vec2::new(-inward.y, inward.x);
    let tri = [
        mid + along * (0.5 * base),
        mid + inward * height,
        mid - along * (0.5 * base),
    ];
    cv.fill_gray(0.82);
    cv.stroke_gray(0.0);
    cv.width(0.25);
    cv.dash(&[]);
    cv.path(&tri, true);
    cv.raw("B");
    cv.fill_gray(0.0);
    if inward.x.abs() > 0.5 {
        let anchor = if inward.x > 0.0 {
            TextAnchor::Start
        } else {
            TextAnchor::End
        };
        cv.text(
            mid.x + inward.x * 2.0,
            mid.y + ts * 0.35,
            code,
            ts,
            anchor,
            true,
            None,
        );
    } else {
        let ty = mid.y + inward.y * (2.0 + if inward.y > 0.0 { ts * CAP } else { 0.0 });
        cv.text(mid.x, ty, code, ts, TextAnchor::Middle, true, None);
    }
}

#[allow(clippy::too_many_arguments)]
fn info_page(
    cv: &mut Canvas,
    ctx: &Ctx,
    lay: &Layout,
    paper: &Paper,
    tiles: &[Tile],
    (cols, rows): (usize, usize),
    pages: &[(String, usize)],
    kind: &str,
    k: f64,
) {
    let (fx, fy, fw, fh) = (paper.fx, paper.fy, paper.fw, paper.fh);
    cv.fill_gray(0.0);
    cv.stroke_gray(0.0);
    cv.text(
        fx,
        fy + 8.0 * k,
        &format!("{} - size {}", ctx.style, ctx.size),
        8.0 * k,
        TextAnchor::Start,
        true,
        None,
    );
    let round = ctx.pattern.raw()["meta"]
        .get("round")
        .filter(|r| !r.is_null())
        .map(|r| format!("  |  round {}", py_scalar(r)))
        .unwrap_or_default();
    let sub = format!(
        "{} {kind}  |  {cols} columns x {rows} rows  |  fabric-pattern/1, mm{round}",
        paper.name
    );
    cv.text(
        fx,
        fy + 14.0 * k,
        &sub,
        3.2 * k,
        TextAnchor::Start,
        false,
        None,
    );
    let top = fy + 20.0 * k;
    draw_test_block(cv, fx, top, true, 3.5);
    let colx = fx + 4.0 * INCH + 8.0;
    let colw = fx + fw - colx;
    let printed: Vec<i64> = tiles
        .iter()
        .filter(|t| !t.blank)
        .map(|t| page_of(pages, &t.name) as i64)
        .collect();
    let blanks: Vec<&Tile> = tiles.iter().filter(|t| t.blank).collect();
    let blank_names: Vec<&str> = blanks.iter().map(|t| t.name.as_str()).collect();
    let ts = 2.9 * k;
    let drop_blank = !ctx.opts.keep_blank;
    let print_line = format!(
        "PRINT PAGES: {} ({} tiles).{}",
        page_list(&printed),
        printed.len(),
        if !blanks.is_empty() && !drop_blank {
            let bp: Vec<i64> = blanks
                .iter()
                .map(|t| page_of(pages, &t.name) as i64)
                .collect();
            format!(
                " Pages {} are blank placeholders (tiles {}): do not print them.",
                page_list(&bp),
                blank_names.join(", ")
            )
        } else if !blanks.is_empty() {
            format!(
                " Blank tiles {} have nothing on them and are omitted from this file.",
                blank_names.join(", ")
            )
        } else {
            String::new()
        }
    );
    let mut all_pages: Vec<i64> = pages.iter().map(|(_, p)| *p as i64).collect();
    all_pages.sort_unstable();
    let stitch = if !(!blanks.is_empty() && drop_blank) {
        format!(
            "STITCHING APPS (Pattern Projector, PDFStitcher): pages {}, {cols} columns, ",
            page_list(&all_pages)
        )
    } else {
        format!(
            "STITCHING APPS (Pattern Projector, PDFStitcher): blank tiles are omitted, so for a {cols}-column grid \
             use the export made with --keep-blank-pages; "
        )
    } + &format!(
        "edge insets {} mm left/right, {} mm top, {} mm bottom.",
        fnum(paper.fx, 3),
        fnum(paper.fy, 3),
        fnum(paper.h - paper.fy - paper.fh, 3)
    );
    let paras = [
        "PRINT AT 100% / ACTUAL SIZE (Scale: None). Do NOT use Fit, Shrink oversized pages or Scale to fit.".to_string(),
        "Print this page first. Measure both squares across AND down: 4 in = 101.6 mm, 10 cm = 100 mm. If either is \
         off by more than 1 mm, fix the printer settings first."
            .to_string(),
        print_line,
        "ASSEMBLE: trim the RIGHT and BOTTOM margins of each page along the frame line, lay it over its right / \
         lower neighbour so the frame lines meet and the half-diamonds form whole diamonds with the same code (e.g. \
         A1|B1). Tape. Pattern lines run 5 mm past the frame so you can check alignment."
            .to_string(),
        "LAYERS: use the Layers panel (Acrobat, Preview, pdf.js) to hide Seam lines, Notches, Grainlines & labels, \
         Placement marks or Soutache placement. One layer per size."
            .to_string(),
        stitch,
        "Seam allowance is included; the cut line is solid, the seam line dashed.".to_string(),
    ];
    let mut yy = top + ts;
    for para in &paras {
        for line in wrap(para, ts, colw) {
            cv.text(colx, yy, &line, ts, TextAnchor::Start, false, None);
            yy += ts * 1.3;
        }
        yy += ts * 0.6;
    }
    cv.width(0.3);
    cv.tag("FPTest1in");
    cv.rect(colx, yy + 2.0, INCH, INCH, "S");
    cv.end_tag();
    cv.text(
        colx + INCH + 3.0,
        yy + 2.0 + INCH / 2.0,
        "1 in",
        ts,
        TextAnchor::Start,
        true,
        None,
    );
    yy += INCH + 8.0;
    cv.text(
        colx,
        yy,
        "Tile map",
        ts * 1.1,
        TextAnchor::Start,
        true,
        None,
    );
    tile_map(
        cv,
        lay,
        paper,
        tiles,
        (cols, rows),
        pages,
        (colx, yy + 2.0, colw, fy + fh - yy - 4.0),
        ts * 0.8,
    );
}

/// Python `str()` of a JSON scalar (`round` values).
fn py_scalar(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n
            .as_i64()
            .map(|i| i.to_string())
            .or_else(|| n.as_f64().map(py::repr_f64))
            .unwrap_or_default(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        other => other.to_string(),
    }
}

/// A finished PDF target: file name, bytes, manifest key and entry.
#[derive(Clone, Debug)]
pub struct PdfFile {
    /// File name.
    pub name: String,
    /// The file.
    pub bytes: Vec<u8>,
    /// Manifest key (`pdf_letter`, ...).
    pub key: String,
    /// Manifest entry.
    pub entry: Value,
}

fn tiles_doc(
    ctx: &Ctx,
    lay: &Layout,
    paper: &Paper,
    kind: &str,
    title: String,
    with_info: bool,
    k: f64,
) -> (PdfDoc, Value) {
    let (cols, rows, tiles) = tile_grid(lay, paper);
    let size_ocg = format!("Size {}", ctx.size);
    let mut ocgs = vec![size_ocg.clone()];
    ocgs.extend(OCG_COMMON.iter().map(|s| (*s).to_string()));
    let mut doc = PdfDoc::new(ocgs, &[], title);
    let drop_blank = !ctx.opts.keep_blank;
    let keep: Vec<&Tile> = tiles.iter().filter(|t| !(t.blank && drop_blank)).collect();
    let first = if with_info { 2 } else { 1 };
    let pages: Vec<(String, usize)> = keep
        .iter()
        .enumerate()
        .map(|(i, t)| (t.name.clone(), first + i))
        .collect();
    let exists: BTreeSet<(usize, usize)> = tiles
        .iter()
        .filter(|t| !t.blank)
        .map(|t| (t.c, t.r))
        .collect();
    if with_info {
        let mut cv = doc.canvas();
        info_page(
            &mut cv,
            ctx,
            lay,
            paper,
            &tiles,
            (cols, rows),
            &pages,
            kind,
            k,
        );
        doc.add_page(paper.w, paper.h, cv, String::new());
    }
    let st = PdfStyle::tiles(k);
    let bleed = 5.0;
    let total = keep.len() + usize::from(with_info);
    for t in &keep {
        let (c, r) = (t.c, t.r);
        let mut cv = doc.canvas();
        let (ox, oy) = (c as f64 * paper.fw, r as f64 * paper.fh);
        if !t.blank {
            cv.fill_gray(0.85);
            let big = paper.fw.min(paper.fh) * 0.22;
            cv.text(
                paper.fx + paper.fw / 2.0,
                paper.fy + paper.fh / 2.0 + big * CAP / 2.0,
                &t.name,
                big,
                TextAnchor::Middle,
                true,
                None,
            );
            cv.save();
            cv.clip_rect(
                paper.fx - bleed,
                paper.fy - bleed,
                paper.fw + 2.0 * bleed,
                paper.fh + 2.0 * bleed,
            );
            cv.raw(format!(
                "1 0 0 1 {} {} cm",
                fnum(paper.fx - ox, 3),
                fnum(paper.fy - oy, 3)
            ));
            let (lo, hi) = (
                Vec2::new(ox - bleed, oy - bleed),
                Vec2::new(ox + paper.fw + bleed, oy + paper.fh + bleed),
            );
            for g in &lay.items {
                if poly_rect_distance(&g.cut, lo, hi) <= 1.0 {
                    draw_piece(&mut cv, g, &size_ocg, &st);
                }
            }
            cv.restore();
        }
        cv.stroke_gray(0.0);
        cv.fill_gray(0.0);
        cv.width(0.3);
        cv.dash(&[]);
        cv.rect(paper.fx, paper.fy, paper.fw, paper.fh, "S");
        let ts = 2.6 * k;
        cv.text(
            paper.fx + 1.5,
            paper.fy + 1.5 + ts,
            &t.name,
            ts * 1.3,
            TextAnchor::Start,
            true,
            None,
        );
        if t.blank {
            cv.text(
                paper.fx + paper.fw / 2.0,
                paper.fy + paper.fh / 2.0,
                &format!("{} - blank, do not print", t.name),
                6.0 * k,
                TextAnchor::Middle,
                true,
                None,
            );
        } else {
            let (fx, fy, fw, fh) = (paper.fx, paper.fy, paper.fw, paper.fh);
            let sides = [
                (
                    (c + 1, Some(r)),
                    Vec2::new(fx + fw, fy + fh / 2.0),
                    Vec2::new(-1.0, 0.0),
                ),
                (
                    (c.wrapping_sub(1), Some(r)),
                    Vec2::new(fx, fy + fh / 2.0),
                    Vec2::new(1.0, 0.0),
                ),
                (
                    (c, r.checked_sub(1)),
                    Vec2::new(fx + fw / 2.0, fy),
                    Vec2::new(0.0, 1.0),
                ),
                (
                    (c, Some(r + 1)),
                    Vec2::new(fx + fw / 2.0, fy + fh),
                    Vec2::new(0.0, -1.0),
                ),
            ];
            for ((nc, nr), mid, inward) in sides {
                let Some(nr) = nr else { continue };
                if !exists.contains(&(nc, nr)) {
                    continue;
                }
                let mut ab = [(c, r), (nc, nr)];
                ab.sort_by_key(|q| (q.1, q.0));
                let code = format!(
                    "{}{}|{}{}",
                    column_letters(ab[0].0),
                    ab[0].1 + 1,
                    column_letters(ab[1].0),
                    ab[1].1 + 1
                );
                half_diamond(&mut cv, mid, inward, &code, 2.5 * k, 30.0 * k, 14.0 * k);
            }
        }
        let foot_y = if paper.band {
            paper.fy + paper.fh + 8.0
        } else {
            paper.fy + paper.fh + 4.0 + ts
        };
        cv.fill_gray(0.0);
        cv.text(
            paper.fx,
            foot_y,
            &format!(
                "{} size {} | tile {} | page {} of {total} | print at 100% (Actual size) | trim right + bottom",
                ctx.style,
                ctx.size,
                t.name,
                page_of(&pages, &t.name)
            ),
            ts,
            TextAnchor::Start,
            false,
            None,
        );
        if paper.band {
            let by = paper.fy + paper.fh + 16.0;
            draw_test_block(&mut cv, paper.fx, by, false, 4.0);
            draw_scale_bars(
                &mut cv,
                paper.fx + 4.0 * INCH + 100.0 + 30.0,
                by + 30.0,
                6.0,
            );
            cv.text(
                paper.fx + 4.0 * INCH + 100.0 + 30.0,
                by + 75.0,
                "Check both squares before cutting. Copyshop: print at 100%, no scaling.",
                5.0,
                TextAnchor::Start,
                false,
                None,
            );
        }
        let extra = format!(
            "/FPTile << /Name {} /Col {c} /Row {r} /Frame [{} {} {} {}] /Origin [{} {}] /Blank {} >> ",
            pdf_string(&t.name),
            fnum(paper.fx, 3),
            fnum(paper.fy, 3),
            fnum(paper.fw, 3),
            fnum(paper.fh, 3),
            fnum(ox, 3),
            fnum(oy, 3),
            t.blank
        );
        doc.add_page(paper.w, paper.h, cv, extra);
    }
    let meta = json!({
        "cols": cols,
        "rows": rows,
        "tiles": tiles.len(),
        "printed": tiles.iter().filter(|t| !t.blank).count(),
        "blank": tiles.iter().filter(|t| t.blank).map(|t| t.name.clone()).collect::<Vec<_>>(),
        "pages": doc.pages.len(),
    });
    (doc, meta)
}

fn entry(kind: &str, file: &str, paper_mm: [f64; 2], meta: &Value) -> Value {
    let mut e = json!({"file": file, "kind": kind, "paper_mm": paper_mm});
    if let (Some(e), Some(m)) = (e.as_object_mut(), meta.as_object()) {
        for (k, v) in m {
            e.insert(k.clone(), v.clone());
        }
    }
    e
}

/// The tiled home-print PDFs `<S>_LETTER.pdf` and `<S>_A4.pdf`.
///
/// # Errors
/// As [`Ctx::master`].
pub fn export_tiled(ctx: &Ctx) -> Result<Vec<PdfFile>, Refusal> {
    let lay = ctx.master()?;
    let mut out = Vec::new();
    for paper in [Paper::letter(), Paper::a4()] {
        let title = format!("{} {} tiles", ctx.style, paper.name);
        let (doc, meta) = tiles_doc(ctx, lay, &paper, "home tiles", title, true, 1.0);
        let name = format!("{}_{}.pdf", ctx.style, paper.name);
        out.push(PdfFile {
            entry: entry("tiled", &name, [paper.w, paper.h], &meta),
            bytes: doc.write(&ctx.created),
            key: format!("pdf_{}", paper.name.to_lowercase()),
            name,
        });
    }
    Ok(out)
}

/// The copy-shop PDFs `<S>_COPYSHOP_A0.pdf` and `<S>_COPYSHOP_36x48in.pdf`: each in the orientation with
/// fewer printed sheets, an information page only when there is more than one.
///
/// # Errors
/// As [`Ctx::master`].
pub fn export_copyshop(ctx: &Ctx) -> Result<Vec<PdfFile>, Refusal> {
    let lay = ctx.master()?;
    let mut out = Vec::new();
    for (label, (w, h)) in [
        ("A0", (841.0, 1189.0)),
        ("36x48in", (36.0 * INCH, 48.0 * INCH)),
    ] {
        let mut best: Option<(usize, Paper)> = None;
        for (ww, hh) in [(w, h), (h, w)] {
            let paper = Paper::copyshop(label, ww, hh);
            let (_, _, tiles) = tile_grid(lay, &paper);
            let n = tiles.iter().filter(|t| !t.blank).count();
            if best.as_ref().is_none_or(|b| n < b.0) {
                best = Some((n, paper));
            }
        }
        let Some((n, paper)) = best else { continue };
        let title = format!("{} copyshop {label}", ctx.style);
        let (doc, meta) = tiles_doc(ctx, lay, &paper, "copyshop", title, n > 1, 2.0);
        let name = format!("{}_COPYSHOP_{label}.pdf", ctx.style);
        out.push(PdfFile {
            entry: entry(
                "copyshop",
                &name,
                [py::round(paper.w, 3), py::round(paper.h, 3)],
                &meta,
            ),
            bytes: doc.write(&ctx.created),
            key: format!("pdf_copyshop_{label}"),
            name,
        });
    }
    Ok(out)
}

fn test_squares_inline(cv: &mut Canvas, x: f64, y: f64, rgb: Option<[f64; 3]>) {
    ink(cv, rgb);
    cv.width(0.6);
    cv.dash(&[]);
    cv.tag("FPTest4in");
    cv.rect(x, y, 4.0 * INCH, 4.0 * INCH, "S");
    cv.end_tag();
    cv.tag("FPTest10cm");
    cv.rect(x + 4.0 * INCH + 8.0, y, 100.0, 100.0, "S");
    cv.end_tag();
    cv.text(
        x + 2.0 * INCH,
        y + 2.0 * INCH,
        "4 in",
        12.0 / CAP,
        TextAnchor::Middle,
        true,
        None,
    );
    cv.text(
        x + 4.0 * INCH + 58.0,
        y + 50.0,
        "10 cm",
        12.0 / CAP,
        TextAnchor::Middle,
        true,
        None,
    );
}

/// The projector PDFs `<S>_PROJECTOR.pdf` and `<S>_PROJECTOR_DARK.pdf`: every piece once, unfolded, grain
/// vertical, at true scale on one page `projector_width` wide, with text at least 12 mm high, test squares and
/// a calibration grid layer (off).
///
/// # Errors
/// `out_of_range` when the page would exceed the PDF limit of 200 in; as [`Ctx::geom`].
pub fn export_projector(ctx: &Ctx) -> Result<Vec<PdfFile>, Refusal> {
    let width = ctx.opts.projector_width;
    let (side, head, foot) = (50.0, 150.0, 100.0);
    let doc_pieces = ctx.pattern.doc.as_ref().ok_or_else(|| {
        Refusal::new(
            RefusalCode::InvalidDocument,
            "the pattern has no typed document",
        )
    })?;
    let mut geoms = Vec::new();
    for piece in &doc_pieces.pieces {
        let inst = primary_instance(ctx.pattern, &piece.id).ok_or_else(|| {
            Refusal::new(
                RefusalCode::InvalidDocument,
                format!("{}: no instance", piece.id),
            )
        })?;
        let mut g = ctx.geom(Source::Instance(inst), 17.0)?;
        g.labels.retain(|lb| lb.kind != "internal");
        let lines = &g.labels[0].lines;
        g.labels[0].lines = vec![lines[0].clone(), lines[2].clone()];
        geoms.push(g);
    }
    let mut lay = shelf_layout(&geoms, 50.0, 0.0, Some(width - 2.0 * side), true, true, 0.0);
    let shift = Affine {
        a: [[1.0, 0.0], [0.0, 1.0]],
        b: Vec2::new(side, head),
    };
    lay.items = lay.items.iter().map(|g| g.transform(&shift)).collect();
    let h = head + lay.height + foot;
    if h >= 200.0 * INCH || width >= 200.0 * INCH {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "projector page {} x {} mm exceeds the 200 in PDF page limit; split the pattern",
                py::fixed(width, 0),
                py::fixed(h, 0)
            ),
        ));
    }
    let size_ocg = format!("Size {}", ctx.size);
    let mut out = Vec::new();
    for dark in [false, true] {
        let mut ocgs = vec![size_ocg.clone()];
        ocgs.extend(OCG_COMMON.iter().map(|s| (*s).to_string()));
        ocgs.push(GRID_OCG.into());
        let title = format!("{} projector{}", ctx.style, if dark { " dark" } else { "" });
        let mut doc = PdfDoc::new(ocgs, &[GRID_OCG], title);
        let mut cv = doc.canvas();
        if dark {
            cv.fill_gray(0.0);
            cv.rect(0.0, 0.0, width, h, "f");
        }
        let rgb = if dark { Some([1.0, 1.0, 1.0]) } else { None };
        let st = PdfStyle {
            cut: 1.0,
            sew: 0.5,
            notch: 1.0,
            thin: 0.6,
            dash: [4.0, 2.0],
            text_scale: 1.0,
            min_text: 12.0 / CAP + 0.05,
            arrow: 20.0,
            ink: rgb,
            cut_ink: if dark { Some([0.0, 1.0, 0.0]) } else { None },
            halo: if dark { 0.0 } else { 1.0 },
        };
        ink(&mut cv, rgb);
        cv.text(
            side,
            30.0,
            &format!(
                "{} size {} - PROJECTOR (true scale, grain vertical)",
                ctx.style, ctx.size
            ),
            st.min_text,
            TextAnchor::Start,
            true,
            None,
        );
        cv.text(
            side,
            30.0 + st.min_text * 1.3,
            "Calibrate to your mat; check the squares.",
            st.min_text,
            TextAnchor::Start,
            false,
            None,
        );
        test_squares_inline(
            &mut cv,
            width - side - (4.0 * INCH + 100.0 + 8.0),
            40.0,
            rgb,
        );
        for g in &lay.items {
            draw_piece(&mut cv, g, &size_ocg, &st);
        }
        cv.begin_oc(GRID_OCG);
        ink(&mut cv, rgb);
        cv.width(0.3);
        for xg in np::arange(0.0, width + 1e-6, 100.0) {
            cv.polyline(&[Vec2::new(xg, 0.0), Vec2::new(xg, h)], false);
        }
        for yg in np::arange(0.0, h + 1e-6, 100.0) {
            cv.polyline(&[Vec2::new(0.0, yg), Vec2::new(width, yg)], false);
        }
        for xi in np::arange(0.0, width, INCH) {
            cv.polyline(&[Vec2::new(xi, 0.0), Vec2::new(xi, 8.0)], false);
        }
        for yi in np::arange(0.0, h, INCH) {
            cv.polyline(&[Vec2::new(0.0, yi), Vec2::new(8.0, yi)], false);
        }
        cv.end_oc();
        doc.add_page(width, h, cv, String::new());
        let name = format!(
            "{}_PROJECTOR{}.pdf",
            ctx.style,
            if dark { "_DARK" } else { "" }
        );
        out.push(PdfFile {
            entry: json!({"file": name, "kind": "projector", "paper_mm": [width, py::round(h, 3)]}),
            bytes: doc.write(&ctx.created),
            key: format!("pdf_projector{}", if dark { "_dark" } else { "" }),
            name,
        });
    }
    Ok(out)
}
