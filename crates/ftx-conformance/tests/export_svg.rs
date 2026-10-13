//! The master SVG against `tests/fixtures/oracle/export/master_svg.json` (captured by
//! `tools/oracle/capture_exports.py`, spec 03 §6, §7.1, §10.4): every fixture pattern, the NOTE piece, and
//! `--unfold` and `--layout-width 300` for TESTBODICE, ROBUST and the 92 cm bolero and trousers. Same lines in
//! the same order with the same elements, ids, classes, attributes and texts; numbers within 0.002 mm; a cut
//! line made by the union fallback within 0.05 mm (Hausdorff); the main label's lines at their own anchor and
//! size (see `export_geoms.rs`). The manifest entry agrees within 0.002 mm.

use ftx_conformance::fixture;
use ftx_core::Vec2;
use ftx_export::{Ctx, ExportOptions, check_svg, master_svg};
use ftx_pattern::Pattern;
use serde_json::Value;

/// A line split into numbers and the text between them.
fn tokens(line: &str) -> (Vec<String>, Vec<f64>) {
    let (mut text, mut nums) = (Vec::new(), Vec::new());
    let b = line.as_bytes();
    let (mut i, mut start) = (0, 0);
    while i < b.len() {
        let neg = b[i] == b'-' && i + 1 < b.len() && b[i + 1].is_ascii_digit();
        let prev_ok = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if (b[i].is_ascii_digit() || neg) && prev_ok {
            let mut j = i + usize::from(neg);
            while j < b.len() && (b[j].is_ascii_digit() || b[j] == b'.') {
                j += 1;
            }
            text.push(line[start..i].to_string());
            nums.push(line[i..j].parse().unwrap_or(f64::NAN));
            start = j;
            i = j;
        } else {
            i += 1;
        }
    }
    text.push(line[start..].to_string());
    (text, nums)
}

/// The points of a line's `d` attribute.
fn ring(line: &str) -> Vec<Vec2> {
    let d = line
        .split("d=\"")
        .nth(1)
        .and_then(|r| r.split('"').next())
        .unwrap_or("");
    let nums: Vec<f64> = d
        .split(|c: char| c == 'M' || c == 'L' || c == 'Z' || c == ',' || c.is_whitespace())
        .filter(|t| !t.is_empty())
        .map(|t| t.parse().unwrap_or(f64::NAN))
        .collect();
    nums.as_chunks::<2>()
        .0
        .iter()
        .map(|c| Vec2::new(c[0], c[1]))
        .collect()
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

#[test]
fn master_svg_matches_the_reference() {
    let fx = fixture("export", "master_svg");
    let mut failures = Vec::new();
    let (mut lines, mut exact, mut cut_union, mut label_lines) = (0, 0, 0, 0);
    let mut worst = 0.0f64;
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let o = &case["input"]["options"];
        let opts = ExportOptions {
            unfold: o["unfold"].as_bool().unwrap_or(false),
            layout_width: o["layout_width"].as_f64(),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let (svg, entry) = master_svg(&ctx).unwrap();
        let want = case["output"]["svg"].as_str().unwrap();
        let wm = &case["output"]["master"];
        for k in ["file", "fold_mode"] {
            if entry[k] != wm[k] {
                failures.push(format!("{id}: master.{k} {} vs {}", entry[k], wm[k]));
            }
        }
        let nums = |v: &Value| -> Vec<f64> {
            match v {
                Value::Array(a) => a.iter().map(|x| x.as_f64().unwrap()).collect(),
                x => vec![x.as_f64().unwrap()],
            }
        };
        for k in ["width_mm", "height_mm", "cut_bbox"] {
            for (g, w) in nums(&entry[k]).iter().zip(nums(&wm[k])) {
                if (g - w).abs() > 0.002 {
                    failures.push(format!("{id}: master.{k} {g} vs {w}"));
                }
            }
        }
        let (got, want): (Vec<&str>, Vec<&str>) = (svg.lines().collect(), want.lines().collect());
        if got.len() != want.len() {
            failures.push(format!("{id}: {} lines vs {}", got.len(), want.len()));
            continue;
        }
        let mut class = String::new();
        for (n, (g, w)) in got.iter().zip(&want).enumerate() {
            lines += 1;
            if let Some(c) = w.split("class=\"").nth(1).and_then(|r| r.split('"').next()) {
                class = c.to_string();
            }
            if g == w {
                exact += 1;
                continue;
            }
            let (gt, gn) = tokens(g);
            let (wt, wn) = tokens(w);
            let at = format!("{id}:{} ({class})", n + 1);
            if class == "cut" && g.starts_with("<path d=\"M") {
                let h = hausdorff(&ring(g), &ring(w));
                if h > 0.05 {
                    failures.push(format!("{at}: cut line {h:.3} mm from the reference's"));
                }
                cut_union += 1;
                continue;
            }
            if gt != wt {
                failures.push(format!("{at}:\n  got  {g}\n  want {w}"));
                continue;
            }
            if w.contains("data-kind=\"label\"") {
                label_lines += 1;
                continue;
            }
            for (a, b) in gn.iter().zip(&wn) {
                worst = worst.max((a - b).abs());
                if (a - b).abs() > 0.002 {
                    failures.push(format!("{at}: {a} vs {b}\n  got  {g}\n  want {w}"));
                    break;
                }
            }
        }
    }
    println!(
        "master SVG: {lines} lines, {exact} identical; the rest equal but for numbers within {worst:.3} mm, {cut_union} \
         union cut lines within 0.05 mm and {label_lines} main-label lines at their own anchor"
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

#[test]
fn svg_checks_pass_and_catch_mutations() {
    let fx = fixture("export", "master_svg");
    let mut n = 0;
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let o = &case["input"]["options"];
        let opts = ExportOptions {
            unfold: o["unfold"].as_bool().unwrap_or(false),
            layout_width: o["layout_width"].as_f64(),
            ..ExportOptions::default()
        };
        let (svg, entry) = master_svg(&Ctx::new(&pat, opts).unwrap()).unwrap();
        let file = entry["file"].as_str().unwrap();
        for (who, text) in [
            ("ftx", svg.as_str()),
            ("reference", case["output"]["svg"].as_str().unwrap()),
        ] {
            for item in check_svg(text, file, false) {
                n += 1;
                assert!(item.ok, "{id} ({who}): {item:?}");
            }
        }
    }
    // the reference's own mutation test (spec 03 §10.1 #11), a piece without its fold group, a wrong viewBox
    let svg = fx["cases"][0]["output"]["svg"].as_str().unwrap();
    let failed = |text: &str| -> Vec<(String, String)> {
        check_svg(text, "x.svg", false)
            .into_iter()
            .filter(|i| !i.ok)
            .map(|i| (i.check, i.detail))
            .collect()
    };
    let t = svg.replacen("<title>", "<g transform=\"scale(1)\"/>\n<title>", 1);
    assert_eq!(
        failed(&t),
        vec![("svg.no_transform".to_string(), String::new())]
    );
    let start = svg
        .find("<g id=\"piece-front_R-fold\" class=\"fold\">")
        .unwrap();
    let end = start + svg[start..].find("</g>").unwrap() + 4;
    let t = format!("{}{}", &svg[..start], &svg[end..]);
    assert_eq!(
        failed(&t),
        vec![(
            "svg.piece_subgroups".to_string(),
            "piece-front_R".to_string()
        )]
    );
    let t = svg.replacen("viewBox=\"0 0 ", "viewBox=\"0 0 1", 1);
    assert_eq!(failed(&t)[0].0, "svg.mm_units_viewbox");
    println!(
        "SVG checks: {n} items pass on ftx's and the reference's SVGs; three mutations each fail their check"
    );
}
