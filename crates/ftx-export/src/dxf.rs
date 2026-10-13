//! DXF (`docs/spec/03` §7.7–7.8): the ASTM D6673 / AAMA pattern exchange files (R12, one block per piece) and the
//! flat DXF for laser beds and plotter software (R2000, millimetres, polylines only). Both are written here, not
//! from a template: R12 with the header variables, tables, blocks and entities the readers need; R2000 with the
//! handles, owners, block records and root dictionary the format requires.

use std::fmt::Write as _;

use ftx_core::{Refusal, Vec2, det, py};

use crate::ctx::{Ctx, primary_instance};
use crate::geom::{CAP, Geom, Source, grain_marks, label_texts};
use crate::layout::shelf_layout;
use crate::pyjson::PyJson;
use crate::text::{Anchor, ascii_text, fnum, safe_name, stroke_text};

/// A DXF being written: group code / value pairs.
#[derive(Default)]
struct Dxf {
    out: String,
}

impl Dxf {
    fn pair(&mut self, code: i32, value: &str) {
        let _ = write!(self.out, "{code:>3}\n{value}\n");
    }
    fn num(&mut self, code: i32, v: f64) {
        self.pair(code, &py::repr_f64(v));
    }
    fn int(&mut self, code: i32, v: i64) {
        self.pair(code, &v.to_string());
    }
    fn point(&mut self, base: i32, x: f64, y: f64, z: f64) {
        self.num(base, x);
        self.num(base + 10, y);
        self.num(base + 20, z);
    }
}

fn r2(p: Vec2) -> (f64, f64) {
    (py::round(p.x, 2), py::round(p.y, 2))
}

/// A ring rounded to 2 decimals, consecutive duplicates (cyclic) dropped.
fn ring2(r: &[Vec2]) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = r.iter().map(|&p| r2(p)).collect();
    let n = pts.len();
    (0..n)
        .filter(|&i| pts[i] != pts[(i + n - 1) % n])
        .map(|i| pts[i])
        .collect()
}

fn unit(v: Vec2) -> Vec2 {
    let n = (v.x * v.x + v.y * v.y).sqrt();
    if n > 0.0 {
        Vec2::new(v.x / n, v.y / n)
    } else {
        v
    }
}

/// Turn points (vertices turning more than `thresh` degrees; the ends of an open line) and curve points (every
/// `spacing` mm along the runs between turns, and each run's farthest point from its chord when over 0.5 mm) for
/// grading software.
#[must_use]
pub fn turn_and_curve_points(
    pts: &[Vec2],
    closed: bool,
    thresh: f64,
    spacing: f64,
) -> (Vec<Vec2>, Vec<Vec2>) {
    let n = pts.len();
    if n < 2 {
        return (Vec::new(), Vec::new());
    }
    let mut turns = Vec::new();
    for i in 0..n {
        if !closed && (i == 0 || i == n - 1) {
            turns.push(i);
            continue;
        }
        let (a, b, c) = (pts[(i + n - 1) % n], pts[i], pts[(i + 1) % n]);
        let (u, v) = (unit(b - a), unit(c - b));
        let ang = det::atan2(u.x * v.y - u.y * v.x, u.x * v.x + u.y * v.y).to_degrees();
        if ang.abs() > thresh {
            turns.push(i);
        }
    }
    if turns.is_empty() {
        turns.push(0);
    }
    let mut curves = Vec::new();
    let mut cyc = turns.clone();
    if closed {
        cyc.push(turns[0] + n);
    }
    for w in cyc.windows(2) {
        let idx: Vec<usize> = (w[0]..=w[1]).map(|k| k % n).collect();
        if idx.len() <= 2 {
            continue;
        }
        let run: Vec<Vec2> = idx.iter().map(|&k| pts[k]).collect();
        let cum = ftx_core::polyline::cumulative(&run);
        let total = cum[cum.len() - 1];
        if total <= 0.0 {
            continue;
        }
        for s in ftx_core::np::arange(spacing, total - spacing * 0.5, spacing) {
            curves.push(ftx_core::polyline::at_fractions(&run, &[s / total])[0]);
        }
        let (a, b) = (run[0], run[run.len() - 1]);
        let d = b - a;
        let len = (d.x * d.x + d.y * d.y).sqrt();
        if len > 0.0 {
            let (mut k, mut best) = (0, f64::NEG_INFINITY);
            for (j, p) in run.iter().enumerate() {
                let dist = ((p.x - a.x) * d.y - (p.y - a.y) * d.x).abs() / len;
                if dist > best {
                    best = dist;
                    k = j;
                }
            }
            if best > 0.5 && k > 0 && k < run.len() - 1 {
                curves.push(run[k]);
            }
        }
    }
    (turns.iter().map(|&i| pts[i]).collect(), curves)
}

fn astm_notch_layer(kind: &str) -> &'static str {
    match kind {
        "T" => "80",
        "castle" => "81",
        "check" => "82",
        "U" => "83",
        _ => "4",
    }
}

fn r12_polyline(d: &mut Dxf, layer: &str, pts: &[(f64, f64)], closed: bool) {
    d.pair(0, "POLYLINE");
    d.pair(8, layer);
    d.int(66, 1);
    d.point(10, 0.0, 0.0, 0.0);
    d.int(70, i64::from(closed));
    for &(x, y) in pts {
        d.pair(0, "VERTEX");
        d.pair(8, layer);
        d.point(10, x, y, 0.0);
    }
    d.pair(0, "SEQEND");
    d.pair(8, layer);
}

fn r12_text(d: &mut Dxf, layer: &str, text: &str, (x, y): (f64, f64), height: f64) {
    d.pair(0, "TEXT");
    d.pair(8, layer);
    d.point(10, x, y, 0.0);
    d.num(40, height);
    d.pair(1, text);
    d.point(11, x, y, 0.0);
}

fn r12_table(d: &mut Dxf, name: &str, entries: &[Vec<(i32, String)>]) {
    d.pair(0, "TABLE");
    d.pair(2, name);
    d.int(70, entries.len() as i64);
    for e in entries {
        d.pair(0, name);
        for (c, v) in e {
            d.pair(*c, v);
        }
    }
    d.pair(0, "ENDTAB");
}

/// The ASTM (`astm: true`) or AAMA R12 DXF `<S>_ASTM_METRIC.dxf` / `<S>_AAMA_METRIC.dxf` and its manifest entry:
/// one block per piece (its primary instance, unfolded) with the cut and sew lines, turn and curve points,
/// notches, grainline, internal lines, drills and piece text; the style text and one INSERT per block in
/// modelspace. `pieces` is the manifest's piece reference (it names the blocks); `created` (`YYYY-MM-DDTHH:MM`)
/// gives the creation date and time text.
///
/// # Errors
/// As [`Ctx::geom`].
pub fn export_dxf_astm(
    ctx: &Ctx,
    pieces: &PyJson,
    astm: bool,
) -> Result<(String, String, PyJson), Refusal> {
    let doc = ctx.pattern.doc.as_ref().ok_or_else(|| {
        Refusal::new(
            ftx_core::RefusalCode::InvalidDocument,
            "the pattern has no typed document",
        )
    })?;
    let names: Vec<String> = match pieces {
        PyJson::Dict(items) => items.iter().map(|(k, _)| k.clone()).collect(),
        _ => Vec::new(),
    };
    let mut layers: Vec<&str> = vec!["1", "2", "3", "4", "7", "8", "11", "13", "14"];
    if astm {
        layers.extend(["15", "80", "81", "82", "83", "84", "87"]);
    }
    let (date, time) = created_date_time(&ctx.created);
    let mut style_text = vec![
        format!("Style Name:{}", ctx.style),
        format!("Creation Date:{date}"),
        format!("Creation Time:{time}"),
        "Author:fabric-pattern;export.py;1.0".to_string(),
        format!("Sample Size:{}", safe_name(&ctx.size, 20)),
        "Grade Rule Table:".to_string(),
        "Units:METRIC".to_string(),
    ];
    if astm {
        style_text.push("ASTM/D13Proposal 1 Version: D 6673-10".into());
        style_text.push("Curve Tolerance:0.05".into());
    }
    let mut blocks = Dxf::default();
    let mut ents = Dxf::default();
    for s in &style_text {
        r12_text(&mut ents, "1", &ascii_text(s), (0.0, 0.0), 2.5);
    }
    let (mut x, mut ext) = (0.0f64, [0.0f64; 4]);
    for (bname, piece) in names.iter().zip(&doc.pieces) {
        let inst = primary_instance(ctx.pattern, &piece.id).ok_or_else(|| {
            Refusal::new(
                ftx_core::RefusalCode::InvalidDocument,
                format!("{}: no instance", piece.id),
            )
        })?;
        let g = ctx.geom(Source::Instance(inst), 10.0)?;
        let b = &mut blocks;
        b.pair(0, "BLOCK");
        b.pair(8, "0");
        b.pair(2, bname);
        b.int(70, 0);
        b.point(10, 0.0, 0.0, 0.0);
        b.pair(3, bname);
        let (cut, sew) = (ring2(&g.cut), ring2(&g.sew));
        r12_polyline(b, "1", &cut, true);
        r12_polyline(b, "14", &sew, true);
        if astm {
            r12_polyline(b, "84", &cut, true);
            r12_polyline(b, "87", &sew, true);
        }
        let (mut tp, mut cp) = (Vec::new(), Vec::new());
        for ring in [&g.cut, &g.sew] {
            let (t, c) = turn_and_curve_points(ring, true, 15.0, 10.0);
            tp.extend(t);
            cp.extend(c);
        }
        for (p, cl) in g.internal_draw.iter().chain(&g.internal_cut) {
            let (t, c) = turn_and_curve_points(p, *cl, 15.0, 10.0);
            tp.extend(t);
            cp.extend(c);
        }
        tp.extend(g.notches.iter().map(|n| n.base));
        for (layer, list) in [("2", &tp), ("3", &cp)] {
            for &p in list.iter() {
                let (px, py_) = r2(p);
                b.pair(0, "POINT");
                b.pair(8, layer);
                b.point(10, px, py_, 0.0);
            }
        }
        for n in &g.notches {
            let ang = py::round(
                det::atan2(n.inward.y, n.inward.x)
                    .to_degrees()
                    .rem_euclid(360.0),
                2,
            );
            let layer = if astm { astm_notch_layer(&n.kind) } else { "4" };
            let (bx, by) = r2(n.base);
            b.pair(0, "POINT");
            b.pair(8, layer);
            b.point(10, bx, by, py::round(n.depth, 2));
            let thickness = py::round(if n.kind == "slit" { 0.0 } else { n.width }, 2);
            if thickness != 0.0 {
                b.num(39, thickness);
            }
            b.num(50, ang);
            let tip = r2(n.base + n.inward * n.depth);
            b.pair(0, "LINE");
            b.pair(8, "4");
            b.point(10, bx, by, 0.0);
            b.point(11, tip.0, tip.1, 0.0);
        }
        if let Some((a, c)) = g.grain {
            let (a, c) = (r2(a), r2(c));
            b.pair(0, "LINE");
            b.pair(8, "7");
            b.point(10, a.0, a.1, 0.0);
            b.point(11, c.0, c.1, 0.0);
        }
        let open = |p: &[Vec2]| p.iter().map(|&q| r2(q)).collect::<Vec<_>>();
        for (p, cl) in &g.internal_draw {
            r12_polyline(b, "8", &if *cl { ring2(p) } else { open(p) }, *cl);
        }
        for (p, _) in &g.fold_lines {
            r12_polyline(b, "8", &open(p), false);
        }
        for d in &g.decor {
            r12_polyline(b, "8", &open(&d.pts), d.closed);
        }
        for (p, cl) in &g.internal_cut {
            r12_polyline(b, "11", &if *cl { ring2(p) } else { open(p) }, *cl);
        }
        for &(p, dia) in &g.drills {
            let (px, py_) = r2(p);
            b.pair(0, "POINT");
            b.pair(8, "13");
            b.point(10, px, py_, py::round(dia, 2));
        }
        let mut ptext = vec![
            format!("Piece Name:{bname}"),
            format!("Size:{}", safe_name(&ctx.size, 20)),
            format!("Quantity:1,{}", i32::from(g.meta.qty == 2)),
        ];
        if let Some(m) = g.meta.material.as_ref().filter(|m| !m.is_empty()) {
            ptext.push(format!("Material:{}", safe_name(m, 40)));
        }
        let lp = g.labels[0].p;
        for (i, s) in ptext.iter().enumerate() {
            r12_text(
                b,
                "1",
                &ascii_text(s),
                (py::round(lp.x, 2), py::round(lp.y - 4.0 * i as f64, 2)),
                2.5,
            );
        }
        if astm {
            let mut notes = vec![ascii_text(&g.name)];
            notes.extend(g.labels[0].lines.get(2).map(|(t, _, _)| ascii_text(t)));
            for d in &g.decor {
                notes.push(format!(
                    "{} {} mm centre line (layer 8)",
                    d.kind,
                    fnum(d.width, 3)
                ));
            }
            for (i, s) in notes.iter().enumerate() {
                let y = lp.y - 4.0 * ptext.len() as f64 - 7.0 * (i + 1) as f64;
                r12_text(b, "15", s, (py::round(lp.x, 2), py::round(y, 2)), 5.0);
            }
            for lb in g.labels.iter().skip(1).filter(|l| l.kind == "internal") {
                r12_text(b, "15", &ascii_text(&lb.lines[0].0), r2(lb.p), 3.5);
            }
        }
        b.pair(0, "ENDBLK");
        b.pair(8, "0");
        let (x0, y0, x1, y1) = g.bbox();
        ents.pair(0, "INSERT");
        ents.pair(8, "1");
        ents.pair(2, bname);
        ents.point(10, py::round(x - x0, 2), py::round(40.0 - y0, 2), 0.0);
        ext = [
            ext[0].min(x),
            ext[1].min(40.0),
            ext[2].max(x + x1 - x0),
            ext[3].max(40.0 + y1 - y0),
        ];
        x += (x1 - x0) + 30.0;
    }
    let mut d = Dxf::default();
    d.pair(0, "SECTION");
    d.pair(2, "HEADER");
    d.pair(9, "$ACADVER");
    d.pair(1, "AC1009");
    d.pair(9, "$INSBASE");
    d.point(10, 0.0, 0.0, 0.0);
    d.pair(9, "$EXTMIN");
    d.point(10, py::round(ext[0], 2), py::round(ext[1], 2), 0.0);
    d.pair(9, "$EXTMAX");
    d.point(10, py::round(ext[2], 2), py::round(ext[3], 2), 0.0);
    d.pair(9, "$CLAYER");
    d.pair(8, "1");
    d.pair(9, "$PDMODE");
    d.int(70, 0);
    d.pair(9, "$PDSIZE");
    d.num(40, 0.0);
    d.pair(9, "$HANDLING");
    d.int(70, 0);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "TABLES");
    let s = |v: &str| v.to_string();
    r12_table(
        &mut d,
        "LTYPE",
        &[vec![
            (2, s("CONTINUOUS")),
            (70, s("0")),
            (3, s("Solid line")),
            (72, s("65")),
            (73, s("0")),
            (40, s("0.0")),
        ]],
    );
    let mut layer_entries = vec![
        vec![
            (2, s("0")),
            (70, s("0")),
            (62, s("7")),
            (6, s("CONTINUOUS")),
        ],
        vec![
            (2, s("Defpoints")),
            (70, s("0")),
            (62, s("7")),
            (6, s("CONTINUOUS")),
        ],
    ];
    layer_entries.extend(
        layers
            .iter()
            .map(|l| vec![(2, s(l)), (70, s("0")), (62, s("7")), (6, s("CONTINUOUS"))]),
    );
    r12_table(&mut d, "LAYER", &layer_entries);
    r12_table(
        &mut d,
        "STYLE",
        &[vec![
            (2, s("STANDARD")),
            (70, s("0")),
            (40, s("0.0")),
            (41, s("1.0")),
            (50, s("0.0")),
            (71, s("0")),
            (42, s("2.5")),
            (3, s("txt")),
            (4, s("")),
        ]],
    );
    r12_table(&mut d, "APPID", &[vec![(2, s("ACAD")), (70, s("0"))]]);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "BLOCKS");
    d.out.push_str(&blocks.out);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "ENTITIES");
    d.out.push_str(&ents.out);
    d.pair(0, "ENDSEC");
    d.pair(0, "EOF");
    let name = format!(
        "{}_{}_METRIC.dxf",
        ctx.style,
        if astm { "ASTM" } else { "AAMA" }
    );
    let entry = PyJson::dict(vec![
        ("file", PyJson::str(&name)),
        ("units", PyJson::str("METRIC")),
        ("blocks", PyJson::Int(names.len() as i64)),
    ]);
    Ok((name, d.out, entry))
}

/// `dd-mm-YYYY` and `HH-MM` from `YYYY-MM-DDTHH:MM` (zeros when absent).
fn created_date_time(created: &str) -> (String, String) {
    let d: Vec<char> = created.chars().filter(char::is_ascii_digit).collect();
    let get = |a: usize, b: usize| -> String {
        (a..b).map(|i| d.get(i).copied().unwrap_or('0')).collect()
    };
    (
        format!("{}-{}-{}", get(6, 8), get(4, 6), get(0, 4)),
        format!("{}-{}", get(8, 10), get(10, 12)),
    )
}

/// Handles for an R2000 file, in hexadecimal from 0x20 up.
struct Handles(u64);

impl Handles {
    fn next(&mut self) -> String {
        self.0 += 1;
        format!("{:X}", self.0)
    }
}

/// The flat R2000 DXF `<S>_flat_mm.dxf` for laser beds, plotter-cutter front ends and craft cutters: every
/// instance laid out (grain vertical, y up), lines on layers CUT, DRAW, NOTCH, DRILL, GRAIN and TEXT (label text
/// as single-stroke polylines: no TEXT entities), millimetres. Its manifest entry.
///
/// # Errors
/// As [`Ctx::geom`].
pub fn export_flat_dxf(ctx: &Ctx) -> Result<(String, String, PyJson), Refusal> {
    let geoms = ctx.instance_geoms(8.0)?;
    let lay = shelf_layout(&geoms, 20.0, 10.0, None, true, false, 0.0);
    let mut h = Handles(0x1F);
    let (model_rec, paper_rec) = (h.next(), h.next());
    let mut ents = Dxf::default();
    let lw = |ents: &mut Dxf, h: &mut Handles, layer: &str, pts: &[(f64, f64)], closed: bool| {
        ents.pair(0, "LWPOLYLINE");
        ents.pair(5, &h.next());
        ents.pair(330, &model_rec);
        ents.pair(100, "AcDbEntity");
        ents.pair(8, layer);
        ents.pair(100, "AcDbPolyline");
        ents.int(90, pts.len() as i64);
        ents.int(70, i64::from(closed));
        for &(x, y) in pts {
            ents.num(10, x);
            ents.num(20, y);
        }
    };
    let open = |p: &[Vec2]| p.iter().map(|&q| r2(q)).collect::<Vec<_>>();
    for g in &lay.items {
        lw(&mut ents, &mut h, "CUT", &ring2(&g.cut), true);
        for (p, cl) in &g.internal_cut {
            lw(&mut ents, &mut h, "CUT", &open(p), *cl);
        }
        lw(&mut ents, &mut h, "DRAW", &ring2(&g.sew), true);
        for (p, cl) in &g.internal_draw {
            lw(&mut ents, &mut h, "DRAW", &open(p), *cl);
        }
        for (p, _) in &g.fold_lines {
            lw(&mut ents, &mut h, "DRAW", &open(p), false);
        }
        for d in &g.decor {
            lw(&mut ents, &mut h, "DRAW", &open(&d.pts), d.closed);
        }
        for n in &g.notches {
            for mk in &n.marks {
                lw(&mut ents, &mut h, "NOTCH", &open(mk), false);
            }
        }
        for &(p, dia) in &g.drills {
            let (x, y) = r2(p);
            ents.pair(0, "CIRCLE");
            ents.pair(5, &h.next());
            ents.pair(330, &model_rec);
            ents.pair(100, "AcDbEntity");
            ents.pair(8, "DRILL");
            ents.pair(100, "AcDbCircle");
            ents.point(10, x, y, 0.0);
            ents.num(40, py::round(dia / 2.0, 3));
        }
        if g.grain.is_some() {
            for mk in grain_marks(g, 8.0) {
                lw(&mut ents, &mut h, "GRAIN", &open(&mk), false);
            }
        }
        for lb in &g.labels {
            for (x, y, t, s, _) in label_texts(lb, false, 0.0, 1.0) {
                for stroke in stroke_text(&t, x, y, s * CAP, Anchor::Middle, false) {
                    lw(&mut ents, &mut h, "TEXT", &open(&stroke), false);
                }
            }
        }
    }
    let mut bb = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for p in lay.items.iter().flat_map(|g: &Geom| g.cut.iter()) {
        bb = [
            bb[0].min(p.x),
            bb[1].min(p.y),
            bb[2].max(p.x),
            bb[3].max(p.y),
        ];
    }
    let mut d = Dxf::default();
    let table =
        |d: &mut Dxf, h: &mut Handles, name: &str, sub: &str, entries: Vec<Vec<(i32, String)>>| {
            let th = h.next();
            d.pair(0, "TABLE");
            d.pair(2, name);
            d.pair(5, &th);
            d.pair(330, "0");
            d.pair(100, "AcDbSymbolTable");
            d.int(70, entries.len() as i64);
            for e in entries {
                d.pair(0, name);
                d.pair(5, &h.next());
                d.pair(330, &th);
                d.pair(100, "AcDbSymbolTableRecord");
                d.pair(100, sub);
                for (c, v) in e {
                    d.pair(c, &v);
                }
            }
            d.pair(0, "ENDTAB");
        };
    let s = |v: &str| v.to_string();
    let mut tables = Dxf::default();
    table(
        &mut tables,
        &mut h,
        "VPORT",
        "AcDbViewportTableRecord",
        vec![vec![
            (2, s("*Active")),
            (70, s("0")),
            (10, s("0.0")),
            (20, s("0.0")),
            (11, s("1.0")),
            (21, s("1.0")),
            (12, py::repr_f64((bb[0] + bb[2]) / 2.0)),
            (22, py::repr_f64((bb[1] + bb[3]) / 2.0)),
            (40, py::repr_f64((bb[3] - bb[1]).max(1.0) * 1.1)),
            (41, s("1.5")),
        ]],
    );
    table(
        &mut tables,
        &mut h,
        "LTYPE",
        "AcDbLinetypeTableRecord",
        ["ByBlock", "ByLayer", "Continuous"]
            .iter()
            .map(|n| {
                vec![
                    (2, s(n)),
                    (70, s("0")),
                    (3, s(if *n == "Continuous" { "Solid line" } else { "" })),
                    (72, s("65")),
                    (73, s("0")),
                    (40, s("0.0")),
                ]
            })
            .collect(),
    );
    let mut layer_rows = vec![vec![
        (2, s("0")),
        (70, s("0")),
        (62, s("7")),
        (6, s("Continuous")),
    ]];
    for (name, color) in [
        ("CUT", 1),
        ("DRAW", 5),
        ("NOTCH", 3),
        ("DRILL", 6),
        ("GRAIN", 4),
        ("TEXT", 7),
    ] {
        layer_rows.push(vec![
            (2, s(name)),
            (70, s("0")),
            (62, color.to_string()),
            (6, s("Continuous")),
        ]);
    }
    table(
        &mut tables,
        &mut h,
        "LAYER",
        "AcDbLayerTableRecord",
        layer_rows,
    );
    table(
        &mut tables,
        &mut h,
        "STYLE",
        "AcDbTextStyleTableRecord",
        vec![vec![
            (2, s("Standard")),
            (70, s("0")),
            (40, s("0.0")),
            (41, s("1.0")),
            (50, s("0.0")),
            (71, s("0")),
            (42, s("2.5")),
            (3, s("txt")),
            (4, s("")),
        ]],
    );
    table(
        &mut tables,
        &mut h,
        "VIEW",
        "AcDbViewTableRecord",
        Vec::new(),
    );
    table(&mut tables, &mut h, "UCS", "AcDbUCSTableRecord", Vec::new());
    table(
        &mut tables,
        &mut h,
        "APPID",
        "AcDbRegAppTableRecord",
        vec![vec![(2, s("ACAD")), (70, s("0"))]],
    );
    table(
        &mut tables,
        &mut h,
        "DIMSTYLE",
        "AcDbDimStyleTableRecord",
        Vec::new(),
    );
    // block records (their handles were taken first, for the entities' owner)
    let br_table = h.next();
    tables.pair(0, "TABLE");
    tables.pair(2, "BLOCK_RECORD");
    tables.pair(5, &br_table);
    tables.pair(330, "0");
    tables.pair(100, "AcDbSymbolTable");
    tables.int(70, 2);
    for (rec, name) in [(&model_rec, "*Model_Space"), (&paper_rec, "*Paper_Space")] {
        tables.pair(0, "BLOCK_RECORD");
        tables.pair(5, rec);
        tables.pair(330, &br_table);
        tables.pair(100, "AcDbSymbolTableRecord");
        tables.pair(100, "AcDbBlockTableRecord");
        tables.pair(2, name);
    }
    tables.pair(0, "ENDTAB");
    let mut blocks = Dxf::default();
    for (rec, name, paper) in [
        (&model_rec, "*Model_Space", false),
        (&paper_rec, "*Paper_Space", true),
    ] {
        blocks.pair(0, "BLOCK");
        blocks.pair(5, &h.next());
        blocks.pair(330, rec);
        blocks.pair(100, "AcDbEntity");
        if paper {
            blocks.int(67, 1);
        }
        blocks.pair(8, "0");
        blocks.pair(100, "AcDbBlockBegin");
        blocks.pair(2, name);
        blocks.int(70, 0);
        blocks.point(10, 0.0, 0.0, 0.0);
        blocks.pair(3, name);
        blocks.pair(1, "");
        blocks.pair(0, "ENDBLK");
        blocks.pair(5, &h.next());
        blocks.pair(330, rec);
        blocks.pair(100, "AcDbEntity");
        if paper {
            blocks.int(67, 1);
        }
        blocks.pair(8, "0");
        blocks.pair(100, "AcDbBlockEnd");
    }
    let (root, groups) = (h.next(), h.next());
    let mut objects = Dxf::default();
    objects.pair(0, "DICTIONARY");
    objects.pair(5, &root);
    objects.pair(330, "0");
    objects.pair(100, "AcDbDictionary");
    objects.int(281, 1);
    objects.pair(3, "ACAD_GROUP");
    objects.pair(350, &groups);
    objects.pair(0, "DICTIONARY");
    objects.pair(5, &groups);
    objects.pair(330, &root);
    objects.pair(100, "AcDbDictionary");
    objects.int(281, 1);
    d.pair(0, "SECTION");
    d.pair(2, "HEADER");
    d.pair(9, "$ACADVER");
    d.pair(1, "AC1015");
    d.pair(9, "$HANDSEED");
    d.pair(5, &format!("{:X}", h.0 + 1));
    d.pair(9, "$INSBASE");
    d.point(10, 0.0, 0.0, 0.0);
    d.pair(9, "$EXTMIN");
    d.point(10, py::round(bb[0], 2), py::round(bb[1], 2), 0.0);
    d.pair(9, "$EXTMAX");
    d.point(10, py::round(bb[2], 2), py::round(bb[3], 2), 0.0);
    d.pair(9, "$MEASUREMENT");
    d.int(70, 1);
    d.pair(9, "$INSUNITS");
    d.int(70, 4);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "CLASSES");
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "TABLES");
    d.out.push_str(&tables.out);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "BLOCKS");
    d.out.push_str(&blocks.out);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "ENTITIES");
    d.out.push_str(&ents.out);
    d.pair(0, "ENDSEC");
    d.pair(0, "SECTION");
    d.pair(2, "OBJECTS");
    d.out.push_str(&objects.out);
    d.pair(0, "ENDSEC");
    d.pair(0, "EOF");
    let name = format!("{}_flat_mm.dxf", ctx.style);
    let entry = PyJson::dict(vec![
        ("file", PyJson::str(&name)),
        ("units", PyJson::str("mm")),
        ("pieces", PyJson::Int(lay.items.len() as i64)),
    ]);
    Ok((name, d.out, entry))
}
