//! HPGL, the cutter files and report, and the manifest's piece reference against
//! `tests/fixtures/oracle/export/targets.json` (captured by `tools/oracle/capture_exports.py`, spec 03 §7.9,
//! §7.10, §7.13, §8): TESTBODICE (also rotated and mirrored, and mirrored), ROBUST and the 92 cm fez, bolero and
//! trousers.
//!
//! - HPGL: the same commands, pens and polylines in the same order; every coordinate within one unit (0.025 mm)
//!   but the main label's strokes (ftx's own anchor and size, same strokes) and a cut line made by the union
//!   fallback (within 0.05 mm, Hausdorff).
//! - Cutter: the same files and titles, outlines within 0.05 mm as regions (GEOS keeps a micrometre sliver
//!   between a notch triangle's base and a curved cut line as a hole), the same report.
//! - The manifest's `pieces`, `hpgl` and `cutter` entries.
//! - ftx's HPGL and cutter checks give exactly the reference checker's items on its files and pass on ftx's.

use ftx_conformance::fixture;
use ftx_core::Vec2;
use ftx_export::{
    Anchor, CAP, Ctx, ExportOptions, check_hpgl, check_svg, export_cutter, export_hpgl,
    grain_marks, label_texts, master_svg, stroke_text,
};
use ftx_pattern::Pattern;
use serde_json::Value;

#[derive(Debug, PartialEq)]
enum Ev {
    Pen(i64),
    Poly(Vec<(i64, i64)>),
    Drill(String),
    Other(String),
}

fn parse_hpgl(text: &str) -> Vec<Ev> {
    let mut out = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    let coords = |s: &str| -> Vec<(i64, i64)> {
        let v: Vec<i64> = s
            .trim_end_matches(';')
            .split(',')
            .filter_map(|t| t.parse().ok())
            .collect();
        v.chunks(2).map(|c| (c[0], c[1])).collect()
    };
    while i < lines.len() {
        let l = lines[i];
        if let Some(n) = l.strip_prefix("SP") {
            out.push(Ev::Pen(n.trim_end_matches(';').parse().unwrap_or(-1)));
        } else if l.contains(";CI") {
            out.push(Ev::Drill(l.to_string()));
        } else if let Some(start) = l.strip_prefix("PU").filter(|s| s.len() > 1) {
            let mut pts = coords(start);
            i += 1;
            while i < lines.len() && lines[i].starts_with("PD") {
                pts.extend(coords(&lines[i][2..]));
                i += 1;
            }
            out.push(Ev::Poly(pts));
            // the closing PU;
        } else {
            out.push(Ev::Other(l.to_string()));
        }
        i += 1;
    }
    out
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

/// Largest distance from a vertex of either polygon to the other's region (0 inside it): the polygons as
/// areas, so a sliver one overlay keeps as a hole and the other as part of the outline does not count.
fn region_distance(a: &[Vec2], b: &[Vec2]) -> f64 {
    let to_ring = |p: Vec2, r: &[Vec2]| {
        (0..r.len())
            .map(|i| {
                let (s, e) = (r[i], r[(i + 1) % r.len()]);
                let d = e - s;
                let l2 = d.x * d.x + d.y * d.y;
                let t = if l2 > 0.0 {
                    (((p.x - s.x) * d.x + (p.y - s.y) * d.y) / l2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                (p - (s + d * t)).norm()
            })
            .fold(f64::INFINITY, f64::min)
    };
    let dir = |a: &[Vec2], b: &[Vec2]| {
        a.iter()
            .map(|&p| {
                if ftx_core::polyline::point_in_polygon(p, b) {
                    0.0
                } else {
                    to_ring(p, b)
                }
            })
            .fold(0.0, f64::max)
    };
    dir(a, b).max(dir(b, a))
}

fn mm(p: &[(i64, i64)]) -> Vec<Vec2> {
    p.iter()
        .map(|&(x, y)| Vec2::new(x as f64 / 40.0, y as f64 / 40.0))
        .collect()
}

fn svg_path(svg: &str) -> Vec<Vec2> {
    let d = svg
        .split("d=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("");
    let nums: Vec<f64> = d
        .split(|c: char| c == 'M' || c == 'L' || c == 'Z' || c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse().ok())
        .collect();
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|c| Vec2::new(c[0], c[1]))
        .collect()
}

fn close(a: &Value, b: &Value, tol: f64) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() <= tol
        }
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q, tol))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && y.iter()
                    .all(|(k, v)| x.get(k).is_some_and(|w| close(w, v, tol)))
        }
        _ => a == b,
    }
}

#[test]
fn hpgl_cutter_and_manifest_match_the_reference() {
    let fx = fixture("export", "targets");
    let mut failures = Vec::new();
    let (mut polys, mut exact_polys, mut label_polys, mut cut_polys, mut items) = (0, 0, 0, 0, 0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let o = &case["input"]["options"];
        let (rot, mir) = (
            o["hpgl_rotate90"].as_bool().unwrap_or(false),
            o["hpgl_mirror"].as_bool().unwrap_or(false),
        );
        let opts = ExportOptions {
            created: case["input"]["created"].as_str().map(str::to_string),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let (_, master) = master_svg(&ctx).unwrap();
        let out = &case["output"];
        let ref_manifest: Value = serde_json::from_str(out["manifest"].as_str().unwrap()).unwrap();

        // HPGL
        let (name, text, hpgl_entry) = export_hpgl(&ctx, rot, mir).unwrap();
        let (got, want) = (parse_hpgl(&text), parse_hpgl(out["hpgl"].as_str().unwrap()));
        if got.len() != want.len() {
            failures.push(format!("{id}: HPGL {} events vs {}", got.len(), want.len()));
        } else {
            // per piece: where its main label's strokes are among its pen-1 polylines
            let lay = ctx.master().unwrap();
            let zones: Vec<(usize, usize)> = lay
                .items
                .iter()
                .map(|g| {
                    let pre = 1
                        + g.internal_draw.len()
                        + g.decor.len()
                        + grain_marks(g, 6.0).len()
                        + g.fold_lines.len();
                    let strokes: usize = label_texts(&g.labels[0], true, 0.0, 1.0)
                        .iter()
                        .map(|(x, y, t, s, _)| {
                            stroke_text(t, *x, *y, s * CAP, Anchor::Middle, true).len()
                        })
                        .sum();
                    (pre, pre + strokes)
                })
                .collect();
            let (mut item, mut pen, mut k1) = (0usize, 1i64, 0usize);
            let mut started = false;
            for (n, (a, b)) in got.iter().zip(&want).enumerate() {
                match (a, b) {
                    (Ev::Pen(p), Ev::Pen(q)) if p == q => {
                        if *p == 1 && started {
                            item += 1;
                        }
                        if *p == 1 {
                            started = true;
                            k1 = 0;
                        }
                        pen = *p;
                    }
                    (Ev::Poly(p), Ev::Poly(q)) => {
                        polys += 1;
                        let is_cut = pen == 2
                            && matches!(got.get(n + 1), Some(Ev::Pen(1)) | Some(Ev::Other(_)));
                        let in_label = pen == 1
                            && zones.get(item).is_some_and(|&(lo, hi)| k1 >= lo && k1 < hi);
                        if pen == 1 {
                            k1 += 1;
                        }
                        if p == q {
                            exact_polys += 1;
                        } else if in_label && p.len() == q.len() {
                            label_polys += 1;
                        } else if is_cut {
                            cut_polys += 1;
                            let h = hausdorff(&mm(p), &mm(q));
                            if h > 0.05 {
                                failures.push(format!("{id}: HPGL cut line {h:.3} mm off"));
                            }
                        } else if p.len() != q.len()
                            || p.iter()
                                .zip(q)
                                .any(|(u, v)| (u.0 - v.0).abs() > 1 || (u.1 - v.1).abs() > 1)
                        {
                            failures.push(format!(
                                "{id}: HPGL polyline {n} (piece {item}, pen {pen}) differs"
                            ));
                        }
                    }
                    (a, b) if a == b => {}
                    (a, b) => failures.push(format!("{id}: HPGL event {n}: {a:?} vs {b:?}")),
                }
            }
        }
        if !close(&hpgl_entry.to_value(), &ref_manifest["hpgl"], 0.002) {
            failures.push(format!(
                "{id}: manifest hpgl {} vs {}",
                hpgl_entry.to_value(),
                ref_manifest["hpgl"]
            ));
        }
        // the checker on the reference's HPGL reproduces the reference's items, and passes on ftx's
        let ref_items: Vec<&Value> = out["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["file"] == name.as_str())
            .collect();
        let on_ref = check_hpgl(
            out["hpgl"].as_str().unwrap(),
            &name,
            Some(&ref_manifest["master"]),
            &ref_manifest["hpgl"],
        );
        for (g, w) in on_ref.iter().zip(&ref_items) {
            items += 1;
            if g.to_json() != **w {
                failures.push(format!(
                    "{id}: HPGL check on the reference's file\n  got  {}\n  want {}",
                    g.to_json(),
                    w
                ));
            }
        }
        if on_ref.len() != ref_items.len() {
            failures.push(format!(
                "{id}: {} HPGL items vs {}",
                on_ref.len(),
                ref_items.len()
            ));
        }
        for item in check_hpgl(&text, &name, Some(&master), &hpgl_entry.to_value()) {
            if !item.ok {
                failures.push(format!("{id}: ftx's HPGL fails {item:?}"));
            }
        }

        // cutter
        let (files, _, report, entry) = export_cutter(&ctx).unwrap();
        let want_files = out["cutter"].as_object().unwrap();
        if files.len() != want_files.len() {
            failures.push(format!(
                "{id}: {} cutter files vs {}",
                files.len(),
                want_files.len()
            ));
        }
        for f in &files {
            let Some(w) = want_files.get(&f.path).and_then(Value::as_str) else {
                failures.push(format!("{id}: no reference cutter file {}", f.path));
                continue;
            };
            let title = |s: &str| {
                s.split("<title>")
                    .nth(1)
                    .and_then(|r| r.split("</title>").next())
                    .map(str::to_string)
            };
            if title(&f.text) != title(w) {
                failures.push(format!("{id}: {} title", f.path));
            }
            let h = region_distance(&svg_path(&f.text), &svg_path(w));
            if h > 0.05 {
                failures.push(format!("{id}: {} outline {h:.3} mm off", f.path));
            }
            let fname = f.path.rsplit('/').next().unwrap();
            for item in check_svg(&f.text, fname, true) {
                if !item.ok {
                    failures.push(format!("{id}: ftx's {} fails {item:?}", f.path));
                }
            }
            let ref_items: Vec<&Value> = out["checks"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|i| i["file"] == fname)
                .collect();
            for (g, w2) in check_svg(w, fname, true).iter().zip(&ref_items) {
                items += 1;
                if g.to_json() != **w2 {
                    failures.push(format!(
                        "{id}: cutter check on the reference's file\n  got  {}\n  want {}",
                        g.to_json(),
                        w2
                    ));
                }
            }
        }
        let (got_report, want_report): (Value, Value) = (
            serde_json::from_str(&report).unwrap(),
            serde_json::from_str(out["cutter_report"].as_str().unwrap()).unwrap(),
        );
        if !close(&got_report, &want_report, 0.1) {
            failures.push(format!(
                "{id}: cutter report\n  got  {got_report}\n  want {want_report}"
            ));
        }
        if !close(&entry.to_value(), &ref_manifest["files"]["cutter"], 0.0) {
            failures.push(format!(
                "{id}: manifest cutter {} vs {}",
                entry.to_value(),
                ref_manifest["files"]["cutter"]
            ));
        }

        // the manifest's piece reference
        let pieces = ctx.piece_reference().unwrap().to_value();
        for (block, w) in ref_manifest["pieces"].as_object().unwrap() {
            let g = &pieces[block];
            let per = w["cut_perimeter"].as_f64().unwrap();
            let ok = g["piece"] == w["piece"]
                && g["instance"] == w["instance"]
                && g["sew_perimeter"] == w["sew_perimeter"]
                && g["sew_area"] == w["sew_area"]
                && (g["cut_perimeter"].as_f64().unwrap() - per).abs() <= 0.05
                && (g["cut_area"].as_f64().unwrap() - w["cut_area"].as_f64().unwrap()).abs()
                    <= 0.05 * per;
            if !ok {
                failures.push(format!(
                    "{id}: manifest piece {block}\n  got  {g}\n  want {w}"
                ));
            }
        }
        if pieces.as_object().unwrap().len() != ref_manifest["pieces"].as_object().unwrap().len() {
            failures.push(format!("{id}: manifest piece count"));
        }
    }
    println!(
        "HPGL/cutter/manifest: {polys} HPGL polylines, {exact_polys} identical, {label_polys} main-label strokes at ftx's \
         anchor, {cut_polys} union cut lines within 0.05 mm; checkers reproduce {items} reference items"
    );
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures
            .iter()
            .take(30)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
