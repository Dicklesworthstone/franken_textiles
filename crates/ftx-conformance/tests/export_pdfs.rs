//! The PDF targets against `tests/fixtures/oracle/export/pdfs.json` and the reference's files in
//! `tests/fixtures/oracle/export/pdf/` (captured by `tools/oracle/capture_exports.py`, spec 03 §7.2–7.6, §8,
//! §10.4): tiled Letter and A4, copy-shop A0 and 36x48in, projector light and dark, for TESTBODICE, ROBUST (also
//! at --layout-width 300 with and without --keep-blank-pages), TESTBODICE at --projector-width 1100 and the
//! 92 cm bolero and trousers.
//!
//! - The same files with the same manifest entries.
//! - ftx's checker on the reference's PDFs gives exactly the reference checker's items (names, verdicts and
//!   details), and passes every item on ftx's own PDFs.
//! - The same objects with the same dictionaries (lengths, dates and producer aside) and, inflated, the same
//!   content lines: numbers within 0.002 mm, a cut line made by the union fallback within 0.05 mm (Hausdorff), the
//!   main label's lines with their own anchor and size.

use ftx_conformance::{fixture, fixture_dir};
use ftx_core::Vec2;
use ftx_export::{
    Ctx, ExportOptions, PdfFile, check_pdf, export_copyshop, export_projector, export_tiled,
    master_svg, pdf_objects, svg_cut_points,
};
use ftx_pattern::Pattern;
use serde_json::Value;

fn polyline_points(line: &str) -> Vec<Vec2> {
    let toks: Vec<&str> = line.split_whitespace().collect();
    let mut pts = Vec::new();
    let mut i = 0;
    while i + 2 < toks.len() + 1 && i + 2 <= toks.len() {
        if i + 2 < toks.len() && (toks[i + 2] == "m" || toks[i + 2] == "l") {
            if let (Ok(x), Ok(y)) = (toks[i].parse(), toks[i + 1].parse()) {
                pts.push(Vec2::new(x, y));
            }
            i += 3;
        } else {
            i += 1;
        }
    }
    pts
}

fn hausdorff(a: &[Vec2], b: &[Vec2]) -> f64 {
    let seg = |p: Vec2, s: Vec2, e: Vec2| {
        let d = e - s;
        let l2 = d.x * d.x + d.y * d.y;
        let t = if l2 > 0.0 {
            (((p.x - s.x) * d.x + (p.y - s.y) * d.y) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        (p - (s + d * t)).norm()
    };
    let dir = |a: &[Vec2], b: &[Vec2]| {
        a.iter()
            .map(|&p| {
                (0..b.len())
                    .map(|i| seg(p, b[i], b[(i + 1) % b.len()]))
                    .fold(f64::INFINITY, f64::min)
            })
            .fold(0.0, f64::max)
    };
    dir(a, b).max(dir(b, a))
}

/// Numbers and the text between them.
fn tokens(line: &str) -> (Vec<String>, Vec<f64>) {
    let (mut text, mut nums) = (Vec::new(), Vec::new());
    let mut cur = String::new();
    for t in line.split(' ') {
        match t.parse::<f64>() {
            Ok(v) if !t.is_empty() => {
                text.push(std::mem::take(&mut cur));
                nums.push(v);
            }
            _ => {
                cur.push_str(t);
                cur.push(' ');
            }
        }
    }
    text.push(cur);
    (text, nums)
}

fn normalize(head: &str) -> String {
    let mut s = head.to_string();
    for (key, close) in [
        ("/Length ", ' '),
        ("/CreationDate (", ')'),
        ("/Producer (", ')'),
    ] {
        if let Some(i) = s.find(key) {
            let start = i + key.len();
            let end = s[start..].find(close).map_or(s.len(), |j| start + j);
            s.replace_range(start..end, "*");
        }
    }
    s
}

#[derive(Default)]
struct Tally {
    lines: usize,
    identical: usize,
    cut: usize,
    label: usize,
    worst: f64,
    failures: Vec<String>,
}

impl Tally {
    fn content(&mut self, at: &str, got: &str, want: &str) {
        let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
        if g.len() != w.len() {
            self.failures
                .push(format!("{at}: {} content lines vs {}", g.len(), w.len()));
            return;
        }
        let mut tags: Vec<String> = Vec::new();
        for (n, (a, b)) in g.iter().zip(&w).enumerate() {
            self.lines += 1;
            if b.ends_with("BDC") || b.ends_with("BMC") {
                tags.push(
                    b.split_whitespace()
                        .rev()
                        .nth(1)
                        .unwrap_or("")
                        .trim_start_matches('/')
                        .to_string(),
                );
            } else if *b == "EMC" {
                tags.pop();
            }
            if a == b {
                self.identical += 1;
                continue;
            }
            let here = format!("{at} line {}", n + 1);
            if (b.ends_with(" h") || b.contains(" h ")) && b.contains(" m ") {
                let h = hausdorff(&polyline_points(a), &polyline_points(b));
                self.cut += 1;
                if h > 0.05 {
                    self.failures.push(format!(
                        "{here}: closed path {h:.3} mm from the reference's"
                    ));
                }
                continue;
            }
            let (at_, an) = tokens(a);
            let (bt, bn) = tokens(b);
            if tags.last().is_some_and(|t| t == "oc3") && b.contains("Tj") {
                self.label += 1;
                let text = |l: &str| l.split('(').nth(1).map(str::to_string);
                if text(a) != text(b) {
                    self.failures
                        .push(format!("{here}: label text\n  got  {a}\n  want {b}"));
                }
                continue;
            }
            if at_ != bt || an.len() != bn.len() {
                self.failures
                    .push(format!("{here}:\n  got  {a}\n  want {b}"));
                continue;
            }
            for (x, y) in an.iter().zip(&bn) {
                self.worst = self.worst.max((x - y).abs());
                if (x - y).abs() > 0.002 {
                    self.failures
                        .push(format!("{here}: {x} vs {y}\n  got  {a}\n  want {b}"));
                    break;
                }
            }
        }
    }
}

#[test]
fn pdfs_match_the_reference() {
    let fx = fixture("export", "pdfs");
    let svgs = fixture("export", "master_svg");
    let svg_of = |doc: &Value, opts: &Value| -> Option<String> {
        svgs["cases"].as_array()?.iter().find_map(|c| {
            let same_opts = c["input"]["options"]["layout_width"] == opts["layout_width"]
                && c["input"]["options"]["unfold"].is_null();
            (c["input"]["pattern"] == *doc && same_opts)
                .then(|| c["output"]["svg"].as_str().unwrap().to_string())
        })
    };
    let mut t = Tally::default();
    let (mut ref_items, mut own_items) = (0, 0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let o = &case["input"]["options"];
        let opts = ExportOptions {
            layout_width: o["layout_width"].as_f64(),
            keep_blank: o["keep_blank"].as_bool().unwrap_or(false),
            projector_width: o["projector_width"].as_f64().unwrap_or(1500.0),
            created: case["input"]["created"].as_str().map(str::to_string),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let (svg, _) = master_svg(&ctx).unwrap();
        let ours_master = svg_cut_points(&svg);
        let ref_svg =
            svg_of(&case["input"]["pattern"], o).expect("a master SVG case for the PDF case");
        let ref_master = svg_cut_points(&ref_svg);
        let mut files: Vec<PdfFile> = export_tiled(&ctx).unwrap();
        files.extend(export_copyshop(&ctx).unwrap());
        files.extend(export_projector(&ctx).unwrap());
        let names: Vec<&str> = files.iter().map(|f| f.name.as_str()).collect();
        let want_names: Vec<&str> = case["output"]["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(names, want_names, "{id}");
        let ref_checks = case["output"]["checks"].as_array().unwrap();
        for f in &files {
            let at = format!("{id}/{}", f.name);
            // manifest entry
            let want = &case["output"]["manifest"][&f.key];
            for (k, v) in want.as_object().unwrap() {
                let g = &f.entry[k];
                let close = match (g.as_f64(), v.as_f64()) {
                    (Some(a), Some(b)) => (a - b).abs() <= 0.002,
                    _ => match (g.as_array(), v.as_array()) {
                        (Some(a), Some(b)) if a.iter().all(Value::is_number) => {
                            a.len() == b.len()
                                && a.iter().zip(b).all(|(x, y)| {
                                    (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() <= 0.002
                                })
                        }
                        _ => g == v,
                    },
                };
                if !close {
                    t.failures.push(format!("{at}: manifest {k} {g} vs {v}"));
                }
            }
            let kind = f.entry["kind"].as_str().unwrap();
            let paper: Vec<f64> = f.entry["paper_mm"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let wp: Vec<f64> = want["paper_mm"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_f64().unwrap())
                .collect();
            let reference =
                std::fs::read(fixture_dir().join("export/pdf").join(id).join(&f.name)).unwrap();
            // the checker on the reference's file reproduces the reference checker
            let got = check_pdf(&reference, &f.name, [wp[0], wp[1]], kind, Some(&ref_master));
            let want: Vec<&Value> = ref_checks
                .iter()
                .filter(|i| i["file"] == f.name.as_str())
                .collect();
            if got.len() != want.len() {
                t.failures.push(format!(
                    "{at}: {} check items on the reference's file vs {}",
                    got.len(),
                    want.len()
                ));
            }
            for (g, w) in got.iter().zip(&want) {
                ref_items += 1;
                if g.to_json() != **w {
                    t.failures.push(format!(
                        "{at}: checker on the reference's file\n  got  {}\n  want {}",
                        g.to_json(),
                        w
                    ));
                }
            }
            // and passes on ftx's own
            for item in check_pdf(
                &f.bytes,
                &f.name,
                [paper[0], paper[1]],
                kind,
                Some(&ours_master),
            ) {
                own_items += 1;
                if !item.ok {
                    t.failures
                        .push(format!("{at}: ftx's file fails {:?}", item));
                }
            }
            // structure and content
            let (a, b) = (pdf_objects(&f.bytes), pdf_objects(&reference));
            if a.len() != b.len() {
                t.failures
                    .push(format!("{at}: {} objects vs {}", a.len(), b.len()));
                continue;
            }
            let latin = |v: &[u8]| v.iter().map(|&c| char::from(c)).collect::<String>();
            for ((na, (ha, sa)), (nb, (hb, sb))) in a.iter().zip(&b) {
                if na != nb || normalize(&latin(ha)) != normalize(&latin(hb)) {
                    t.failures.push(format!(
                        "{at}: object {na}\n  got  {}\n  want {}",
                        latin(ha),
                        latin(hb)
                    ));
                    continue;
                }
                if let (Some(sa), Some(sb)) = (sa, sb) {
                    t.content(&format!("{at} obj {na}"), &latin(sa), &latin(sb));
                }
            }
        }
    }
    println!(
        "PDFs: checker reproduces {ref_items} reference items; {own_items} items pass on ftx's files; {} content lines, \
         {} identical, the rest numbers within {:.3} mm, {} closed paths within 0.05 mm, {} main-label text lines",
        t.lines, t.identical, t.worst, t.cut, t.label
    );
    assert!(
        t.failures.is_empty(),
        "{} failures:\n{}",
        t.failures.len(),
        t.failures
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn pdf_checks_catch_mutations() {
    let fx = fixture("export", "pdfs");
    let case = &fx["cases"][0];
    let pat = Pattern::new(case["input"]["pattern"].clone());
    let opts = ExportOptions {
        created: case["input"]["created"].as_str().map(str::to_string),
        ..ExportOptions::default()
    };
    let ctx = Ctx::new(&pat, opts).unwrap();
    let (svg, _) = master_svg(&ctx).unwrap();
    let master = svg_cut_points(&svg);
    let letter = export_tiled(&ctx).unwrap().remove(0);
    let failed = |paper: [f64; 2], pts: &[(f64, f64)]| -> Vec<String> {
        check_pdf(&letter.bytes, &letter.name, paper, "tiled", Some(pts))
            .into_iter()
            .filter(|i| !i.ok)
            .map(|i| i.check)
            .collect()
    };
    assert!(failed([215.9, 279.4], &master).is_empty());
    // the wrong paper
    assert_eq!(
        failed([210.0, 297.0], &master),
        vec!["pdf.mediabox_exact".to_string()]
    );
    // master vertices 1 mm off: the tiles no longer restitch onto them
    let shifted: Vec<(f64, f64)> = master.iter().map(|&(x, y)| (x + 1.0, y)).collect();
    assert_eq!(
        failed([215.9, 279.4], &shifted),
        vec!["pdf.restitch_matches_master".to_string()]
    );
    // a master vertex off every tile
    let mut beyond = master.clone();
    beyond.push((1e5, 1e5));
    assert_eq!(
        failed([215.9, 279.4], &beyond),
        vec!["pdf.tiles_cover_master".to_string()]
    );
}
