//! Acceptance checks (`docs/spec/03` §8): each output file read back on its own. So far the SVG and PDF
//! checks.

use std::collections::{BTreeMap, BTreeSet};

use ftx_core::py;
use serde_json::{Value, json};

use crate::xml::parse;

/// One check of one file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CheckItem {
    /// Check name (`svg.no_transform`, ...).
    pub check: String,
    /// File name.
    pub file: String,
    /// Passed.
    pub ok: bool,
    /// What was measured.
    pub detail: String,
}

impl CheckItem {
    fn new(check: &str, file: &str, ok: bool, detail: impl Into<String>) -> Self {
        Self {
            check: check.into(),
            file: file.into(),
            ok,
            detail: detail.into(),
        }
    }

    /// `{check, file, ok, detail}`.
    #[must_use]
    pub fn to_json(&self) -> Value {
        json!({"check": self.check, "file": self.file, "ok": self.ok, "detail": self.detail})
    }
}

/// The checks of the master SVG (`cutter: false`) or of a cutter SVG: no `transform` anywhere, millimetre
/// width and height matching the viewBox, and every piece group with all its sub-groups (master) or a single
/// closed path (cutter).
#[must_use]
pub fn check_svg(text: &str, file: &str, cutter: bool) -> Vec<CheckItem> {
    let mut out = vec![CheckItem::new(
        "svg.no_transform",
        file,
        !text.contains("transform"),
        "",
    )];
    let root = parse(text).unwrap_or_default();
    let attr = |k: &str| root.get(k).unwrap_or("").to_string();
    let (w, h, vb) = (attr("width"), attr("height"), attr("viewBox"));
    let mut ok = w.ends_with("mm") && h.ends_with("mm");
    if ok {
        let v: Option<Vec<f64>> = vb.split_whitespace().map(|q| q.parse().ok()).collect();
        let num = |s: &str| s[..s.len() - 2].trim().parse::<f64>().ok();
        ok = match (v, num(&w), num(&h)) {
            (Some(v), Some(wv), Some(hv)) => {
                v.len() == 4
                    && v[0] == 0.0
                    && v[1] == 0.0
                    && (v[2] - wv).abs() < 1e-6
                    && (v[3] - hv).abs() < 1e-6
            }
            _ => false,
        };
    }
    out.push(CheckItem::new(
        "svg.mm_units_viewbox",
        file,
        ok,
        format!("{w} x {h}, viewBox {vb}"),
    ));
    if cutter {
        let paths: Vec<_> = root
            .descendants()
            .into_iter()
            .filter(|e| e.name == "path")
            .collect();
        out.push(CheckItem::new(
            "cutter.single_closed_path",
            file,
            paths.len() == 1 && paths[0].get("d").unwrap_or("").trim_end().ends_with('Z'),
            "",
        ));
    } else {
        const NEED: [&str; 10] = [
            "cut",
            "sew",
            "notches",
            "grain",
            "fold",
            "internal-draw",
            "internal-cut",
            "drills",
            "labels",
            "decor-soutache",
        ];
        let bad: Vec<String> = root
            .children
            .iter()
            .filter(|g| g.name == "g" && g.get("id").unwrap_or("").starts_with("piece-"))
            .filter(|g| {
                let have: Vec<&str> = g
                    .children
                    .iter()
                    .filter(|c| c.name == "g")
                    .filter_map(|c| c.get("class"))
                    .collect();
                !NEED.iter().all(|n| have.contains(n))
            })
            .map(|g| g.get("id").unwrap_or("").to_string())
            .collect();
        out.push(CheckItem::new(
            "svg.piece_subgroups",
            file,
            bad.is_empty(),
            bad.join(", "),
        ));
    }
    out
}

/// The objects of a PDF written by the PDF writer: number → (dictionary text, inflated stream). A sequential
/// scan, streams skipped by `/Length` (spec 03 §8.3).
#[must_use]
pub fn pdf_objects(data: &[u8]) -> BTreeMap<u32, (Vec<u8>, Option<Vec<u8>>)> {
    let mut objs = BTreeMap::new();
    let mut pos = 0;
    while let Some(off) = find(data, b" 0 obj\n", pos) {
        let mut ds = off;
        while ds > 0 && data[ds - 1].is_ascii_digit() {
            ds -= 1;
        }
        if ds == off {
            pos = off + 1;
            continue;
        }
        let num: u32 = std::str::from_utf8(&data[ds..off])
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let start = off + 7;
        let sm = find(data, b"stream\n", start);
        let em = find(data, b"endobj", start).unwrap_or(data.len());
        match sm {
            Some(sm) if sm < em => {
                let head = data[start..sm].to_vec();
                let len = find(&head, b"/Length ", 0)
                    .map(|i| {
                        head[i + 8..]
                            .iter()
                            .take_while(|b| b.is_ascii_digit())
                            .fold(0usize, |a, b| a * 10 + usize::from(b - b'0'))
                    })
                    .unwrap_or(0);
                let end = (sm + 7 + len).min(data.len());
                let raw = &data[sm + 7..end];
                let stream = if find(&head, b"/FlateDecode", 0).is_some() {
                    miniz_oxide::inflate::decompress_to_vec_zlib(raw).unwrap_or_default()
                } else {
                    raw.to_vec()
                };
                objs.insert(num, (head, Some(stream)));
                pos = find(data, b"endobj", end).map_or(data.len(), |e| e + 6);
            }
            _ => {
                objs.insert(num, (data[start..em].to_vec(), None));
                pos = em + 6;
            }
        }
    }
    objs
}

fn find(hay: &[u8], needle: &[u8], from: usize) -> Option<usize> {
    if from >= hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|w| w == needle)
        .map(|i| i + from)
}

/// The name of an OCG dictionary (escapes removed).
fn pdf_ocg_name(head: &[u8]) -> Option<String> {
    let i = find(head, b"/Type /OCG /Name (", 0)? + 18;
    let mut out = Vec::new();
    let mut j = i;
    while j < head.len() && head[j] != b')' {
        if head[j] == b'\\' && j + 1 < head.len() {
            out.push(head[j + 1]);
            j += 2;
        } else {
            out.push(head[j]);
            j += 1;
        }
    }
    Some(out.iter().map(|&b| char::from(b)).collect())
}

#[derive(Clone, Debug, PartialEq)]
enum Tok {
    Str(String),
    Op(String),
    Name(String),
    Num(f64),
}

fn pdf_tokens(data: &[u8]) -> Vec<Tok> {
    const DELIM: &[u8] = b"()<>[]{}/% \t\r\n\x0c\x00";
    let (mut i, n) = (0, data.len());
    let mut out = Vec::new();
    let latin = |b: &[u8]| b.iter().map(|&c| char::from(c)).collect::<String>();
    while i < n {
        let c = data[i];
        if b" \t\r\n\x0c\x00".contains(&c) {
            i += 1;
        } else if c == b'%' {
            while i < n && data[i] != b'\r' && data[i] != b'\n' {
                i += 1;
            }
        } else if c == b'(' {
            let (mut depth, mut j, mut buf) = (1, i + 1, Vec::new());
            while j < n && depth > 0 {
                let ch = data[j];
                if ch == b'\\' && j + 1 < n {
                    buf.push(data[j + 1]);
                    j += 2;
                    continue;
                }
                if ch == b'(' {
                    depth += 1;
                } else if ch == b')' {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                }
                buf.push(ch);
                j += 1;
            }
            out.push(Tok::Str(latin(&buf)));
            i = j + 1;
        } else if data[i..].starts_with(b"<<") || data[i..].starts_with(b">>") {
            out.push(Tok::Op(latin(&data[i..i + 2])));
            i += 2;
        } else if c == b'[' || c == b']' {
            out.push(Tok::Op(char::from(c).to_string()));
            i += 1;
        } else if c == b'/' {
            let mut j = i + 1;
            while j < n && !DELIM.contains(&data[j]) {
                j += 1;
            }
            out.push(Tok::Name(latin(&data[i + 1..j])));
            i = j;
        } else {
            let mut j = i;
            while j < n && !DELIM.contains(&data[j]) {
                j += 1;
            }
            let tok = latin(&data[i..j]);
            i = j.max(i + 1);
            out.push(tok.parse::<f64>().map_or(Tok::Op(tok), Tok::Num));
        }
    }
    out
}

type Ctm = [f64; 6];

fn mul(m: Ctm, n: Ctm) -> Ctm {
    let [a, b, c, d, e, f] = m;
    let [aa, bb, cc, dd, ee, ff] = n;
    [
        a * aa + b * cc,
        a * bb + b * dd,
        c * aa + d * cc,
        c * bb + d * dd,
        e * aa + f * cc + ee,
        e * bb + f * dd + ff,
    ]
}

/// Rectangles (with the tags around them and their size in page points), points of paths tagged `FPCut` (page
/// points) and effective font sizes (page points) of a content stream.
type Scan = (Vec<(Vec<String>, f64, f64)>, Vec<(f64, f64)>, Vec<f64>);

fn content_scan(stream: &[u8]) -> Scan {
    let mut ctm: Ctm = [1.0, 0.0, 0.0, 1.0, 0.0, 0.0];
    let (mut stack, mut tags, mut ops): (Vec<Ctm>, Vec<String>, Vec<Tok>) =
        (Vec::new(), Vec::new(), Vec::new());
    let (mut rects, mut cutpts, mut fonts) = (Vec::new(), Vec::new(), Vec::new());
    let mut tm_scale = 1.0;
    for tok in pdf_tokens(stream) {
        let op = match &tok {
            Tok::Op(o) if !matches!(o.as_str(), "[" | "]" | "<<" | ">>") => o.clone(),
            _ => {
                ops.push(tok);
                continue;
            }
        };
        let args: Vec<f64> = ops
            .iter()
            .filter_map(|t| if let Tok::Num(v) = t { Some(*v) } else { None })
            .collect();
        let last = |k: usize| -> Vec<f64> { args[args.len().saturating_sub(k)..].to_vec() };
        match op.as_str() {
            "q" => stack.push(ctm),
            "Q" => ctm = stack.pop().unwrap_or(ctm),
            "cm" if args.len() >= 6 => {
                let a = last(6);
                ctm = mul([a[0], a[1], a[2], a[3], a[4], a[5]], ctm);
            }
            "BMC" | "BDC" => {
                let names: Vec<&String> = ops
                    .iter()
                    .filter_map(|t| match t {
                        Tok::Name(s) | Tok::Str(s) => Some(s),
                        _ => None,
                    })
                    .collect();
                if let Some(first) = names.first() {
                    tags.push(if op == "BMC" || first.as_str() != "OC" {
                        (*first).clone()
                    } else {
                        format!("OC:{}", names.get(1).map_or("", |s| s.as_str()))
                    });
                }
            }
            "EMC" => {
                tags.pop();
            }
            "re" if args.len() >= 4 => {
                let a = last(4);
                let sx = ctm[0].hypot(ctm[1]);
                let sy = ctm[2].hypot(ctm[3]);
                rects.push((tags.clone(), a[2].abs() * sx, a[3].abs() * sy));
            }
            "m" | "l" if tags.iter().any(|t| t == "FPCut") && args.len() >= 2 => {
                let a = last(2);
                cutpts.push((
                    a[0] * ctm[0] + a[1] * ctm[2] + ctm[4],
                    a[0] * ctm[1] + a[1] * ctm[3] + ctm[5],
                ));
            }
            "Tm" if args.len() >= 6 => {
                let a = last(6);
                tm_scale = a[0].hypot(a[1]);
            }
            "Tf" if !args.is_empty() => {
                let size = args[args.len() - 1];
                fonts.push(size * tm_scale * (ctm[0] * ctm[3] - ctm[1] * ctm[2]).abs().sqrt());
            }
            _ => {}
        }
        ops.clear();
    }
    (rects, cutpts, fonts)
}

/// Largest distance from a point of `pts` to its nearest point of `reference` (a 1 mm hash grid searched in
/// rings 0..3; nothing within → infinite).
fn nearest_max(pts: &[(f64, f64)], reference: &[(f64, f64)]) -> f64 {
    let mut grid: BTreeMap<(i64, i64), Vec<usize>> = BTreeMap::new();
    for (i, &(x, y)) in reference.iter().enumerate() {
        grid.entry((x.floor() as i64, y.floor() as i64))
            .or_default()
            .push(i);
    }
    let mut worst = 0.0f64;
    for &(x, y) in pts {
        let (cx, cy) = (x.floor() as i64, y.floor() as i64);
        let mut best = f64::INFINITY;
        for r in 0..4i64 {
            for gx in cx - r..=cx + r {
                for gy in cy - r..=cy + r {
                    if (gx - cx).abs().max((gy - cy).abs()) != r {
                        continue;
                    }
                    for &i in grid.get(&(gx, gy)).map_or(&[][..], Vec::as_slice) {
                        best = best.min((reference[i].0 - x).hypot(reference[i].1 - y));
                    }
                }
            }
            if best <= r as f64 {
                break;
            }
        }
        worst = worst.max(best);
    }
    worst
}

/// The vertices of the cut paths of a master SVG (sheet mm).
#[must_use]
pub fn svg_cut_points(svg: &str) -> Vec<(f64, f64)> {
    let Some(root) = parse(svg) else {
        return Vec::new();
    };
    let mut pts = Vec::new();
    for g in root.descendants() {
        if g.name == "g" && g.get("class") == Some("cut") {
            for p in g.descendants().into_iter().filter(|e| e.name == "path") {
                let d = p.get("d").unwrap_or("");
                let nums: Vec<f64> = numbers(d);
                pts.extend(nums.as_chunks::<2>().0.iter().map(|c| (c[0], c[1])));
            }
        }
    }
    pts
}

/// `-?\d+(?:\.\d+)?` matches of a string as numbers.
fn numbers(s: &str) -> Vec<f64> {
    let b = s.as_bytes();
    let (mut out, mut i) = (Vec::new(), 0);
    while i < b.len() {
        let neg = b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit();
        if b[i].is_ascii_digit() || neg {
            let mut j = i + usize::from(neg);
            while j < b.len() && b[j].is_ascii_digit() {
                j += 1;
            }
            if j + 1 < b.len() && b[j] == b'.' && b[j + 1].is_ascii_digit() {
                j += 1;
                while j < b.len() && b[j].is_ascii_digit() {
                    j += 1;
                }
            }
            out.push(s[i..j].parse().unwrap_or(f64::NAN));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

/// The test squares found on a page, as Python prints the dictionary.
fn squares_repr(sq: &[(String, (f64, f64))]) -> String {
    let items: Vec<String> = sq
        .iter()
        .map(|(k, (w, h))| format!("'{k}': ({}, {})", py::repr_f64(*w), py::repr_f64(*h)))
        .collect();
    format!("{{{}}}", items.join(", "))
}

fn text_after<'a>(head: &'a str, key: &str, close: char) -> Option<&'a str> {
    let i = head.find(key)? + key.len();
    let j = head[i..].find(close)? + i;
    Some(&head[i..j])
}

/// The checks of one PDF (spec 03 §8.2): print scaling off, unique layers, exact media boxes, the same layers
/// on every page, the test squares (page 1 of a tiled file, every sheet of a copy-shop file, the projector's),
/// projector text at least 12 mm high and the page under 200 in, and with the master SVG's cut vertices that
/// every tiled or copy-shop cut point maps back onto them within 0.2 mm and every master vertex lies on a
/// printed tile.
#[must_use]
pub fn check_pdf(
    data: &[u8],
    file: &str,
    expect_mm: [f64; 2],
    kind: &str,
    master_pts: Option<&[(f64, f64)]>,
) -> Vec<CheckItem> {
    const K: f64 = 72.0 / 25.4;
    let mut out = Vec::new();
    let objs = pdf_objects(data);
    let heads: Vec<(u32, String, Option<&Vec<u8>>)> = objs
        .iter()
        .map(|(n, (h, s))| (*n, h.iter().map(|&b| char::from(b)).collect(), s.as_ref()))
        .collect();
    let cat = heads.iter().find(|(_, h, _)| h.contains("/Type /Catalog"));
    out.push(CheckItem::new(
        "pdf.print_scaling_none",
        file,
        cat.is_some_and(|(_, h, _)| h.contains("/PrintScaling /None")),
        "",
    ));
    let ocg_names: Vec<String> = objs.values().filter_map(|(h, _)| pdf_ocg_name(h)).collect();
    let unique: BTreeSet<&String> = ocg_names.iter().collect();
    out.push(CheckItem::new(
        "pdf.ocg_unique",
        file,
        unique.len() == ocg_names.len() && !ocg_names.is_empty(),
        ocg_names.join(", "),
    ));
    let is_page = |h: &str| {
        h.match_indices("/Type /Page").any(|(i, _)| {
            h[i + 11..]
                .chars()
                .next()
                .is_none_or(|c| !(c.is_alphanumeric() || c == '_'))
        }) && !h.contains("/Type /Pages")
    };
    let pages: Vec<&(u32, String, Option<&Vec<u8>>)> =
        heads.iter().filter(|(_, h, _)| is_page(h)).collect();
    let (ew, eh) = (expect_mm[0] * K, expect_mm[1] * K);
    let mut props = BTreeSet::new();
    let (mut mb_ok, mut sq_pages, mut tiles_info, mut fonts_all) =
        (true, Vec::new(), Vec::new(), Vec::new());
    for (_, h, _) in &pages {
        let mb: Vec<f64> = text_after(h, "/MediaBox [", ']')
            .map(|s| {
                s.split_whitespace()
                    .filter_map(|v| v.parse().ok())
                    .collect()
            })
            .unwrap_or_default();
        if !(mb.len() == 4
            && (mb[2] - ew).abs() < 0.01
            && (mb[3] - eh).abs() < 0.01
            && mb[0] == 0.0
            && mb[1] == 0.0)
        {
            mb_ok = false;
        }
        props.insert(
            text_after(h, "/Properties << ", '>')
                .unwrap_or("")
                .to_string(),
        );
        let cnum: u32 = text_after(h, "/Contents ", ' ')
            .and_then(|s| s.parse().ok())
            .unwrap_or(0);
        let stream = objs
            .get(&cnum)
            .and_then(|o| o.1.clone())
            .unwrap_or_default();
        let (rects, cutpts, fonts) = content_scan(&stream);
        fonts_all.extend(fonts);
        let mut sq: Vec<(String, (f64, f64))> = Vec::new();
        for (tags, w, hh) in &rects {
            for t in ["FPTest4in", "FPTest10cm"] {
                if tags.iter().any(|x| x == t) {
                    let v = (w / K, hh / K);
                    match sq.iter_mut().find(|(k, _)| k == t) {
                        Some(e) => e.1 = v,
                        None => sq.push((t.to_string(), v)),
                    }
                }
            }
        }
        sq_pages.push(sq);
        if let Some(i) = h.find("/FPTile << /Name (") {
            let t = &h[i..];
            let name = text_after(t, "/Name (", ')').unwrap_or("").to_string();
            let fr: Vec<f64> = text_after(t, "/Frame [", ']')
                .map(|s| {
                    s.split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            let org: Vec<f64> = text_after(t, "/Origin [", ']')
                .map(|s| {
                    s.split_whitespace()
                        .filter_map(|v| v.parse().ok())
                        .collect()
                })
                .unwrap_or_default();
            let blank = t.contains("/Blank true");
            let mbh = mb.get(3).copied().unwrap_or(0.0);
            if fr.len() == 4 && org.len() == 2 {
                let pts: Vec<(f64, f64)> = cutpts
                    .iter()
                    .map(|&(x, y)| (x / K - fr[0] + org[0], (mbh - y) / K - fr[1] + org[1]))
                    .collect();
                tiles_info.push((name, fr, org, blank, pts));
            }
        }
    }
    out.push(CheckItem::new(
        "pdf.mediabox_exact",
        file,
        mb_ok && !pages.is_empty(),
        format!("expected {ew:.4} x {eh:.4} pt, {} pages", pages.len()),
    ));
    out.push(CheckItem::new(
        "pdf.ocg_identical_across_pages",
        file,
        props.len() == 1,
        "",
    ));
    let sq_ok = |sq: &[(String, (f64, f64))]| {
        let get = |k: &str| sq.iter().find(|(n, _)| n == k).map(|(_, v)| *v);
        matches!((get("FPTest4in"), get("FPTest10cm")), (Some(a), Some(b))
            if (a.0 - 101.6).abs() < 0.05 && (a.1 - 101.6).abs() < 0.05 && (b.0 - 100.0).abs() < 0.05 && (b.1 - 100.0).abs() < 0.05)
    };
    let first = sq_pages.first().cloned().unwrap_or_default();
    match kind {
        "tiled" => out.push(CheckItem::new(
            "pdf.test_squares_page1",
            file,
            sq_ok(&first),
            squares_repr(&first),
        )),
        "copyshop" => {
            let n = sq_pages.iter().filter(|s| sq_ok(s)).count();
            out.push(CheckItem::new(
                "pdf.test_squares_every_sheet",
                file,
                n == sq_pages.len(),
                format!("{n}/{}", sq_pages.len()),
            ));
        }
        "projector" => {
            out.push(CheckItem::new(
                "pdf.test_squares",
                file,
                sq_ok(&first),
                squares_repr(&first),
            ));
            let min = fonts_all.iter().copied().fold(f64::INFINITY, f64::min);
            let cap = min / K * crate::geom::CAP;
            out.push(CheckItem::new(
                "pdf.projector_text_cap_12mm",
                file,
                !fonts_all.is_empty() && cap >= 12.0 - 1e-3,
                if fonts_all.is_empty() {
                    "no text".to_string()
                } else {
                    format!("min cap height {cap:.2} mm")
                },
            ));
            out.push(CheckItem::new(
                "pdf.projector_under_200in",
                file,
                expect_mm[0].max(expect_mm[1]) < 200.0 * 25.4,
                "",
            ));
        }
        _ => {}
    }
    if matches!(kind, "tiled" | "copyshop")
        && let Some(master) = master_pts.filter(|m| !m.is_empty())
    {
        let mut worst = 0.0f64;
        let mut covered = vec![false; master.len()];
        for (_, fr, org, blank, pts) in &tiles_info {
            let (fw, fh) = (fr[2], fr[3]);
            if !blank {
                for (c, &(x, y)) in covered.iter_mut().zip(master) {
                    *c |= x >= org[0] - 1e-6
                        && x <= org[0] + fw + 1e-6
                        && y >= org[1] - 1e-6
                        && y <= org[1] + fh + 1e-6;
                }
            }
            if !pts.is_empty() {
                worst = worst.max(nearest_max(pts, master));
            }
        }
        out.push(CheckItem::new(
            "pdf.restitch_matches_master",
            file,
            worst <= 0.2,
            format!(
                "max deviation {worst:.4} mm over {} tiles",
                tiles_info.len()
            ),
        ));
        let n = covered.iter().filter(|c| **c).count();
        out.push(CheckItem::new(
            "pdf.tiles_cover_master",
            file,
            n == master.len(),
            format!("{n}/{} master cut vertices on printed tiles", master.len()),
        ));
    }
    out
}

fn py_tuple(v: &[f64]) -> String {
    let items: Vec<String> = v.iter().map(|&x| py::repr_f64(x)).collect();
    format!("({})", items.join(", "))
}

/// The HPGL checks: the manifest's `hpgl.cut_bbox` matches the master's (when there is one), there are pen-2
/// (cut) coordinates, and their bounding box / 40 matches the master cut bounding box flipped to y up (only its
/// area when the plot is rotated or mirrored). `master` is the manifest's `master` entry, `hpgl` its `hpgl` entry.
#[must_use]
pub fn check_hpgl(text: &str, file: &str, master: Option<&Value>, hpgl: &Value) -> Vec<CheckItem> {
    let mut out = Vec::new();
    let nums = |v: &Value| -> Vec<f64> {
        v.as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    let (bbox, height) = match master {
        None => (
            nums(&hpgl["cut_bbox"]),
            nums(&hpgl["sheet_mm"]).get(1).copied().unwrap_or(0.0),
        ),
        Some(m) => {
            let ok = hpgl.get("cut_bbox").is_none()
                || nums(&hpgl["cut_bbox"])
                    .iter()
                    .zip(nums(&m["cut_bbox"]))
                    .all(|(a, b)| (a - b).abs() <= 0.01);
            out.push(CheckItem::new("hpgl.same_layout_as_master", file, ok, ""));
            (nums(&m["cut_bbox"]), m["height_mm"].as_f64().unwrap_or(0.0))
        }
    };
    let (mut pen, mut xs, mut ys) = (0i64, Vec::new(), Vec::new());
    for cmd in text.replace('\n', "").split(';') {
        if cmd.len() < 2 {
            continue;
        }
        let (op, args) = cmd.split_at(2);
        if op == "SP" {
            pen = args.parse().unwrap_or(0);
        } else if (op == "PU" || op == "PD") && !args.is_empty() && pen == 2 {
            let v: Vec<i64> = args.split(',').filter_map(|t| t.parse().ok()).collect();
            xs.extend(v.iter().step_by(2).copied());
            ys.extend(v.iter().skip(1).step_by(2).copied());
        }
    }
    if xs.is_empty() || bbox.len() < 4 {
        out.push(CheckItem::new(
            "hpgl.bbox",
            file,
            false,
            "no SP2 coordinates",
        ));
        return out;
    }
    let (x0, y0, x1, y1) = (bbox[0], bbox[1], bbox[2], bbox[3]);
    let exp = [x0, height - y1, x1, height - y0];
    let mn = |v: &[i64]| *v.iter().min().unwrap_or(&0) as f64 / 40.0;
    let mx = |v: &[i64]| *v.iter().max().unwrap_or(&0) as f64 / 40.0;
    let got = [mn(&xs), mn(&ys), mx(&xs), mx(&ys)];
    let turned = hpgl
        .get("rotate_90")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || hpgl.get("mirror").and_then(Value::as_bool).unwrap_or(false);
    let ok = if turned {
        ((got[2] - got[0]) * (got[3] - got[1]) - (x1 - x0) * (y1 - y0)).abs()
            < 0.1 * (x1 - x0 + y1 - y0)
    } else {
        got.iter().zip(&exp).all(|(a, b)| (a - b).abs() <= 0.1)
    };
    out.push(CheckItem::new(
        "hpgl.bbox_matches_master",
        file,
        ok,
        format!(
            "plt {} vs master {}",
            py_tuple(&got.map(|v| py::round(v, 3))),
            py_tuple(&exp.map(|v| py::round(v, 3)))
        ),
    ));
    out
}

/// A DXF entity: its type, its group pairs and, for a POLYLINE or LWPOLYLINE, its points and closed flag.
#[derive(Clone, Debug, Default)]
struct DxfEntity {
    kind: String,
    pairs: Vec<(i32, String)>,
    points: Vec<(f64, f64)>,
    closed: bool,
}

impl DxfEntity {
    fn get(&self, code: i32) -> Option<&str> {
        self.pairs
            .iter()
            .find(|(c, _)| *c == code)
            .map(|(_, v)| v.as_str())
    }
    fn layer(&self) -> &str {
        self.get(8).unwrap_or("")
    }
}

/// What the checks need of a DXF: its version, layer names, blocks with their entities and the modelspace
/// entities.
#[derive(Clone, Debug, Default)]
struct DxfDoc {
    version: String,
    layers: Vec<String>,
    blocks: Vec<(String, Vec<DxfEntity>)>,
    modelspace: Vec<DxfEntity>,
}

fn read_dxf(text: &str) -> DxfDoc {
    let lines: Vec<&str> = text.lines().collect();
    let pairs: Vec<(i32, &str)> = lines
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| {
            (
                c[0].trim().parse().unwrap_or(-1),
                c[1].trim_end_matches('\r'),
            )
        })
        .collect();
    let mut doc = DxfDoc::default();
    let (mut i, n) = (0, pairs.len());
    let mut section = String::new();
    let mut entities: Vec<DxfEntity> = Vec::new();
    while i < n {
        let (code, value) = pairs[i];
        if code == 0 && value == "SECTION" {
            section = pairs.get(i + 1).map_or(String::new(), |p| p.1.to_string());
            i += 2;
            continue;
        }
        if code == 0 && value == "ENDSEC" {
            if section == "ENTITIES" {
                doc.modelspace = std::mem::take(&mut entities);
            }
            section.clear();
            i += 1;
            continue;
        }
        if code == 9 && value == "$ACADVER" && section == "HEADER" {
            doc.version = pairs
                .get(i + 1)
                .map_or(String::new(), |p| p.1.trim().to_string());
            i += 2;
            continue;
        }
        if code == 0 {
            // an entity (or table entry, block marker): gather its pairs
            let mut e = DxfEntity {
                kind: value.to_string(),
                ..DxfEntity::default()
            };
            i += 1;
            while i < n && pairs[i].0 != 0 {
                e.pairs.push((pairs[i].0, pairs[i].1.trim().to_string()));
                i += 1;
            }
            match (section.as_str(), e.kind.as_str()) {
                ("TABLES", "LAYER") => {
                    if let Some(name) = e.get(2) {
                        doc.layers.push(name.to_string());
                    }
                }
                ("BLOCKS", "BLOCK") => doc
                    .blocks
                    .push((e.get(2).unwrap_or("").to_string(), Vec::new())),
                ("BLOCKS", "ENDBLK") => {}
                ("BLOCKS" | "ENTITIES", "VERTEX") => {
                    let target = if section == "BLOCKS" {
                        doc.blocks.last_mut().map(|b| &mut b.1)
                    } else {
                        Some(&mut entities)
                    };
                    if let Some(last) = target.and_then(|v| v.last_mut()) {
                        let x = e.get(10).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                        let y = e.get(20).and_then(|v| v.parse().ok()).unwrap_or(0.0);
                        last.points.push((x, y));
                    }
                }
                ("BLOCKS" | "ENTITIES", "SEQEND") => {}
                ("BLOCKS" | "ENTITIES", _) => {
                    let flags: i64 = e.get(70).and_then(|v| v.parse().ok()).unwrap_or(0);
                    e.closed = flags & 1 == 1;
                    if e.kind == "LWPOLYLINE" {
                        let xs: Vec<f64> = e
                            .pairs
                            .iter()
                            .filter(|(c, _)| *c == 10)
                            .filter_map(|(_, v)| v.parse().ok())
                            .collect();
                        let ys: Vec<f64> = e
                            .pairs
                            .iter()
                            .filter(|(c, _)| *c == 20)
                            .filter_map(|(_, v)| v.parse().ok())
                            .collect();
                        e.points = xs.into_iter().zip(ys).collect();
                    }
                    if section == "BLOCKS" {
                        if let Some(b) = doc.blocks.last_mut() {
                            b.1.push(e);
                        }
                    } else {
                        entities.push(e);
                    }
                }
                _ => {}
            }
            continue;
        }
        i += 1;
    }
    doc
}

fn area_perimeter(pts: &[(f64, f64)]) -> (f64, f64) {
    let ring: Vec<ftx_core::Vec2> = pts
        .iter()
        .map(|&(x, y)| ftx_core::Vec2::new(x, y))
        .collect();
    let n = ring.len();
    let lengths: Vec<f64> = (0..n)
        .map(|i| {
            let d = ring[(i + 1) % n] - ring[i];
            (d.x * d.x + d.y * d.y).sqrt()
        })
        .collect();
    (
        ftx_core::polyline::signed_area(&ring).abs(),
        ftx_core::np::pairwise_sum(&lengths),
    )
}

/// The checks of an ASTM or AAMA R12 DXF (spec 03 §8.2): ASCII, no LWPOLYLINE, version AC1009, one block per
/// manifest piece with its INSERT, the style text, and per block its name, closed layer-1 cut line, single
/// grainline, piece text, and cut and sew line perimeters and areas against the manifest's references (`sew_ref`
/// recomputed from the pattern replaces the manifest's sew values when given).
#[must_use]
pub fn check_dxf(
    raw: &[u8],
    file: &str,
    reference: &Value,
    sew_ref: Option<&Value>,
) -> Vec<CheckItem> {
    let mut out = vec![
        CheckItem::new("dxf.ascii", file, raw.iter().all(|&b| b < 128), ""),
        CheckItem::new(
            "dxf.no_lwpolyline",
            file,
            find(raw, b"LWPOLYLINE", 0).is_none(),
            "",
        ),
    ];
    let text: String = raw.iter().map(|&b| char::from(b)).collect();
    let doc = read_dxf(&text);
    out.push(CheckItem::new(
        "dxf.r12",
        file,
        doc.version == "AC1009",
        doc.version.clone(),
    ));
    let blocks: Vec<&(String, Vec<DxfEntity>)> = doc
        .blocks
        .iter()
        .filter(|(n, _)| !(n.starts_with('*') || n.starts_with('$')))
        .collect();
    let empty = serde_json::Map::new();
    let refs = reference.as_object().unwrap_or(&empty);
    out.push(CheckItem::new(
        "dxf.block_count",
        file,
        blocks.len() == refs.len(),
        format!("{} blocks vs {} pieces", blocks.len(), refs.len()),
    ));
    let mut inserts: Vec<String> = doc
        .modelspace
        .iter()
        .filter(|e| e.kind == "INSERT")
        .map(|e| e.get(2).unwrap_or("").to_string())
        .collect();
    let n_inserts = inserts.len();
    inserts.sort();
    let mut bnames: Vec<String> = blocks.iter().map(|(n, _)| n.clone()).collect();
    bnames.sort();
    out.push(CheckItem::new(
        "dxf.inserts",
        file,
        inserts == bnames,
        format!("{n_inserts} INSERTs"),
    ));
    let texts: Vec<&str> = doc
        .modelspace
        .iter()
        .filter(|e| e.kind == "TEXT")
        .filter_map(|e| e.get(1))
        .collect();
    out.push(CheckItem::new(
        "dxf.style_text",
        file,
        texts.contains(&"Units:METRIC") && texts.iter().any(|t| t.starts_with("Style Name:")),
        "",
    ));
    for (name, ents) in blocks {
        out.push(CheckItem::new(
            "dxf.block_name",
            file,
            name.chars().count() <= 20 && name.is_ascii(),
            name.clone(),
        ));
        let l1: Vec<&DxfEntity> = ents
            .iter()
            .filter(|e| e.kind == "POLYLINE" && e.layer() == "1")
            .collect();
        out.push(CheckItem::new(
            "dxf.layer1_closed",
            file,
            !l1.is_empty() && l1.iter().all(|e| e.closed),
            format!("{name}: {} layer-1 polylines", l1.len()),
        ));
        let l7 = ents
            .iter()
            .filter(|e| e.kind == "LINE" && e.layer() == "7")
            .count();
        out.push(CheckItem::new(
            "dxf.one_grain_line",
            file,
            l7 == 1,
            format!("{name}: {l7}"),
        ));
        let bt: Vec<&str> = ents
            .iter()
            .filter(|e| e.kind == "TEXT")
            .filter_map(|e| e.get(1))
            .collect();
        let pn: Vec<&&str> = bt.iter().filter(|t| t.starts_with("Piece Name:")).collect();
        out.push(CheckItem::new(
            "dxf.piece_text",
            file,
            pn.len() == 1 && pn[0][11..].chars().count() <= 20 && bt.iter().all(|t| t.is_ascii()),
            name.clone(),
        ));
        let Some(r) = refs.get(name).filter(|_| !l1.is_empty()) else {
            out.push(CheckItem::new(
                "dxf.reference",
                file,
                false,
                format!("{name}: no reference"),
            ));
            continue;
        };
        let cut = l1
            .iter()
            .map(|e| &e.points)
            .max_by(|a, b| area_perimeter(a).0.total_cmp(&area_perimeter(b).0))
            .cloned()
            .unwrap_or_default();
        let (a, p) = area_perimeter(&cut);
        let f = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
        let (rp, ra) = (f(&r["cut_perimeter"]), f(&r["cut_area"]));
        out.push(CheckItem::new(
            "dxf.cut_perimeter",
            file,
            (p - rp).abs() <= 0.1,
            format!("{name}: {p:.3} vs {rp:.3}"),
        ));
        out.push(CheckItem::new(
            "dxf.cut_area",
            file,
            (a - ra).abs() <= 0.1 * rp,
            format!("{name}: {a:.1} vs {ra:.1} mm2"),
        ));
        let sref = sew_ref.and_then(|s| s.get(name)).unwrap_or(r);
        match ents
            .iter()
            .find(|e| e.kind == "POLYLINE" && e.layer() == "14")
        {
            Some(l14) => {
                let (a, p) = area_perimeter(&l14.points);
                let (sp, sa) = (f(&sref["sew_perimeter"]), f(&sref["sew_area"]));
                out.push(CheckItem::new(
                    "dxf.sew_perimeter",
                    file,
                    (p - sp).abs() <= 0.1,
                    format!("{name}: {p:.3} vs model {sp:.3}"),
                ));
                out.push(CheckItem::new(
                    "dxf.sew_area",
                    file,
                    (a - sa).abs() <= 0.1 * sp,
                    format!("{name}: {a:.1} vs model {sa:.1}"),
                ));
            }
            None => out.push(CheckItem::new(
                "dxf.sew_line",
                file,
                false,
                format!("{name}: no layer-14 sew line"),
            )),
        }
    }
    out
}

/// The checks of the flat DXF: its layers, closed CUT polylines and no TEXT or MTEXT entities.
#[must_use]
pub fn check_flat_dxf(text: &str, file: &str) -> Vec<CheckItem> {
    let doc = read_dxf(text);
    let need = ["CUT", "DRAW", "NOTCH", "DRILL", "GRAIN", "TEXT"];
    let cuts: Vec<&DxfEntity> = doc
        .modelspace
        .iter()
        .filter(|e| e.kind == "LWPOLYLINE" && e.layer() == "CUT")
        .collect();
    vec![
        CheckItem::new(
            "flatdxf.layers",
            file,
            need.iter().all(|n| doc.layers.iter().any(|l| l == n)),
            "",
        ),
        CheckItem::new(
            "flatdxf.cut_closed",
            file,
            !cuts.is_empty() && cuts.iter().any(|e| e.closed),
            format!("{} CUT polylines", cuts.len()),
        ),
        CheckItem::new(
            "flatdxf.no_text_entities",
            file,
            !doc.modelspace
                .iter()
                .any(|e| e.kind == "TEXT" || e.kind == "MTEXT"),
            "",
        ),
    ]
}

/// The GarmentCode checks: centimetres with relative curvature, closed edge loops, and stitches naming existing
/// panel edges, none twice.
#[must_use]
pub fn check_garmentcode(text: &str, file: &str) -> Vec<CheckItem> {
    let spec: Value = serde_json::from_str(text).unwrap_or(Value::Null);
    let props = &spec["properties"];
    let mut out = vec![CheckItem::new(
        "gc.units_cm_relative",
        file,
        props["units_in_meter"].as_i64() == Some(100) && props["curvature_coords"] == "relative",
        "",
    )];
    let empty = serde_json::Map::new();
    let panels = spec["pattern"]["panels"].as_object().unwrap_or(&empty);
    let closed = panels.values().all(|p| {
        let e = p["edges"].as_array().map(Vec::as_slice).unwrap_or_default();
        (0..e.len()).all(|i| e[i]["endpoints"][1] == e[(i + 1) % e.len()]["endpoints"][0])
    });
    out.push(CheckItem::new("gc.edge_loops_closed", file, closed, ""));
    let stitches = spec["pattern"]["stitches"]
        .as_array()
        .map(Vec::as_slice)
        .unwrap_or_default();
    let mut used = BTreeSet::new();
    let mut ok = true;
    for st in stitches {
        for side in st
            .as_array()
            .map(Vec::as_slice)
            .unwrap_or_default()
            .iter()
            .take(2)
        {
            let (p, e) = (
                side["panel"].as_str().unwrap_or(""),
                side["edge"].as_i64().unwrap_or(-1),
            );
            let n = panels
                .get(p)
                .and_then(|q| q["edges"].as_array())
                .map_or(0, Vec::len) as i64;
            ok &= panels.contains_key(p)
                && (0..n).contains(&e)
                && !used.contains(&(p.to_string(), e));
            used.insert((p.to_string(), e));
        }
    }
    out.push(CheckItem::new(
        "gc.stitches_valid_unique",
        file,
        ok,
        format!("{} stitches", stitches.len()),
    ));
    out
}

fn seg_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let l2 = dx * dx + dy * dy;
    let t = if l2 > 0.0 {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / l2).clamp(0.0, 1.0)
    } else {
        0.0
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// The checks of a soutache DST (spec 03 §8.2) against its manifest entry `info`: the header, one END and
/// nothing after, the record count, the extents, one stop, stitch lengths, the placement and couching lengths,
/// the couching inside the placement line; with the instance's braid lines (pattern mm, `braid`) every couching
/// stitch on one of them and their whole length sewn.
#[must_use]
pub fn check_dst(
    data: &[u8],
    file: &str,
    info: &Value,
    braid: Option<&[Vec<(f64, f64)>]>,
) -> Vec<CheckItem> {
    let d = crate::dst::read_dst(data);
    let get = |k: &str| d.head.iter().find(|(n, _)| n == k).map(|(_, v)| v.clone());
    let mut keys: Vec<String> = d.head.iter().map(|(k, _)| k.clone()).collect();
    keys.sort();
    keys.dedup();
    let fields: Vec<String> = keys.iter().map(|k| format!("'{k}'")).collect();
    let mut out = vec![
        CheckItem::new(
            "dst.header",
            file,
            ["LA", "ST", "CO", "+X", "-X", "+Y", "-Y"]
                .iter()
                .all(|k| get(k).is_some())
                && d.size >= 512,
            format!("fields [{}]", fields.join(", ")),
        ),
        CheckItem::new(
            "dst.ends_once",
            file,
            d.ended,
            "one END record, nothing after it",
        ),
    ];
    let st = get("ST").unwrap_or_default();
    let st_ok = !st.is_empty()
        && st.chars().all(|c| c.is_ascii_digit())
        && st.parse::<usize>().ok() == Some(d.moves.len());
    out.push(CheckItem::new(
        "dst.count",
        file,
        st_ok,
        format!(
            "ST {} vs {}",
            get("ST").unwrap_or_else(|| "None".into()),
            d.moves.len()
        ),
    ));
    let (mut xs, mut ys, mut x, mut y) = (vec![0i64], vec![0i64], 0i64, 0i64);
    let (mut stitched, mut place_len, mut couch_len) = (Vec::new(), 0.0, 0.0);
    let (mut stops, mut place_pts, mut couch_pts) = (0, Vec::new(), Vec::new());
    for &(dx, dy, kind) in &d.moves {
        x += dx;
        y += dy;
        xs.push(x);
        ys.push(y);
        if kind == "stop" {
            stops += 1;
        } else if kind == "stitch" {
            let s = ftx_core::det::hypot(dx as f64, dy as f64) * 0.1;
            stitched.push(s);
            if stops > 0 {
                couch_len += s;
                couch_pts.push((x as f64 * 0.1, y as f64 * 0.1));
            } else {
                place_len += s;
                place_pts.push((x as f64 * 0.1, y as f64 * 0.1));
            }
        }
    }
    let ext = [
        *xs.iter().max().unwrap_or(&0),
        -*xs.iter().min().unwrap_or(&0),
        *ys.iter().max().unwrap_or(&0),
        -*ys.iter().min().unwrap_or(&0),
    ];
    let declared: Vec<Option<i64>> = ["+X", "-X", "+Y", "-Y"]
        .iter()
        .map(|k| {
            let v = get(k).unwrap_or_default();
            let digits = v.trim_start_matches(['+', '-']);
            (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
                .then(|| v.parse().ok())
                .flatten()
        })
        .collect();
    let tuple = |v: &[Option<i64>]| {
        let items: Vec<String> = v
            .iter()
            .map(|x| x.map_or("None".to_string(), |n| n.to_string()))
            .collect();
        format!("({})", items.join(", "))
    };
    out.push(CheckItem::new(
        "dst.extents",
        file,
        ext.iter().map(|&v| Some(v)).collect::<Vec<_>>() == declared,
        format!(
            "decoded {}, header {}",
            tuple(&ext.map(Some)),
            tuple(&declared)
        ),
    ));
    out.push(CheckItem::new(
        "dst.one_stop",
        file,
        stops == 1 && get("CO").is_some_and(|c| c.trim() == "1"),
        format!("{stops} stop(s)"),
    ));
    let moving: Vec<f64> = stitched.iter().copied().filter(|&s| s > 0.0).collect();
    let (mn, mx) = (
        moving.iter().copied().fold(f64::INFINITY, f64::min),
        moving.iter().copied().fold(f64::NEG_INFINITY, f64::max),
    );
    out.push(CheckItem::new(
        "dst.stitch_length",
        file,
        !moving.is_empty() && mn >= 0.5 && mx <= 121.0 * 0.1 + 1e-9,
        format!(
            "{:.2}-{:.2} mm",
            if moving.is_empty() { 0.0 } else { mn },
            if moving.is_empty() { 0.0 } else { mx }
        ),
    ));
    let repr = |v: &Value| v.as_f64().map_or_else(|| v.to_string(), py::repr_f64);
    let num = |v: &Value| v.as_f64().unwrap_or(f64::NAN);
    out.push(CheckItem::new(
        "dst.placement_length",
        file,
        (place_len - num(&info["placement_mm"])).abs() <= 0.05,
        format!("{place_len:.2} vs {} mm", repr(&info["placement_mm"])),
    ));
    out.push(CheckItem::new(
        "dst.couch_length",
        file,
        (couch_len - num(&info["couch_mm"])).abs() <= 0.05,
        format!("{couch_len:.2} vs {} mm", repr(&info["couch_mm"])),
    ));
    let ring: Vec<ftx_core::Vec2> = place_pts
        .iter()
        .map(|&(a, b)| ftx_core::Vec2::new(a, b))
        .collect();
    let inside = place_pts.len() > 2
        && couch_pts.iter().all(|&p| {
            ftx_core::polyline::point_in_polygon(ftx_core::Vec2::new(p.0, p.1), &ring)
                || (0..place_pts.len())
                    .map(|i| seg_distance(p, place_pts[i], place_pts[(i + 1) % place_pts.len()]))
                    .fold(f64::INFINITY, f64::min)
                    < 0.5
        });
    out.push(CheckItem::new(
        "dst.braid_inside_placement",
        file,
        inside,
        format!("{} couching stitches", couch_pts.len()),
    ));
    if let Some(braid) = braid {
        let c = info["centre_mm"]
            .as_array()
            .map(|a| (a[0].as_f64().unwrap_or(0.0), a[1].as_f64().unwrap_or(0.0)))
            .unwrap_or((0.0, 0.0));
        let lines: Vec<Vec<(f64, f64)>> = braid
            .iter()
            .map(|b| b.iter().map(|&(x, y)| (x - c.0, y - c.1)).collect())
            .collect();
        let far = couch_pts
            .iter()
            .map(|&p| {
                lines
                    .iter()
                    .flat_map(|l| l.windows(2).map(move |w| seg_distance(p, w[0], w[1])))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(f64::NEG_INFINITY, f64::max);
        let far = if couch_pts.is_empty() {
            f64::INFINITY
        } else {
            far
        };
        out.push(CheckItem::new(
            "dst.on_braid_lines",
            file,
            far <= 0.2,
            format!("farthest couching stitch {far:.3} mm from a drafted braid line"),
        ));
        let drafted: f64 = lines
            .iter()
            .map(|l| {
                l.windows(2)
                    .map(|w| (w[1].0 - w[0].0).hypot(w[1].1 - w[0].1))
                    .sum::<f64>()
            })
            .sum();
        out.push(CheckItem::new(
            "dst.covers_braid",
            file,
            couch_len >= 0.99 * drafted,
            format!("sewn {couch_len:.1} of {drafted:.1} mm drafted"),
        ));
    }
    out
}
