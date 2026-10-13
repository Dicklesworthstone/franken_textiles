//! The ASTM, AAMA and flat DXFs against the reference's files in `tests/fixtures/oracle/export/targets.json`
//! (captured by `tools/oracle/capture_exports.py` with the clocks fixed; spec 03 §7.7–7.8, §8): TESTBODICE, ROBUST
//! and the 92 cm bolero and trousers.
//!
//! - ASTM/AAMA: the same blocks in the same order and, per block and layer, the same entities: coordinates within
//!   the 2-decimal rounding, but a cut line made by the union fallback (and the turn and curve points taken from
//!   it) within 0.05 mm, and piece texts that hang off the main label's anchor compared by their strings; the same
//!   modelspace text and INSERTs.
//! - Flat: the same entities in the same order; the main label's stroke text at ftx's own anchor; union cut lines
//!   within 0.05 mm.
//! - ftx's DXF checks give exactly the reference checker's items on its files and pass on ftx's.

use ftx_conformance::fixture;
use ftx_core::Vec2;
use ftx_export::{
    Anchor, CAP, Ctx, ExportOptions, Method, check_dxf, check_flat_dxf, cut_outline,
    export_dxf_astm, export_flat_dxf, label_texts, primary_instance, stroke_text,
};
use ftx_pattern::Pattern;
use serde_json::Value;

#[derive(Clone, Debug, Default)]
struct Ent {
    kind: String,
    layer: String,
    nums: Vec<(i32, f64)>,
    text: String,
    pts: Vec<(f64, f64)>,
}

/// Blocks (name, entities) and modelspace entities; POLYLINE vertices and LWPOLYLINE points gathered.
fn parse(text: &str) -> (Vec<(String, Vec<Ent>)>, Vec<Ent>) {
    let lines: Vec<&str> = text.lines().collect();
    let pairs: Vec<(i32, &str)> = lines
        .chunks(2)
        .filter(|c| c.len() == 2)
        .map(|c| (c[0].trim().parse().unwrap_or(-1), c[1].trim()))
        .collect();
    let (mut blocks, mut ms): (Vec<(String, Vec<Ent>)>, Vec<Ent>) = (Vec::new(), Vec::new());
    let mut section = "";
    let mut i = 0;
    while i < pairs.len() {
        let (code, value) = pairs[i];
        if code == 0 && value == "SECTION" {
            section = pairs.get(i + 1).map_or("", |p| p.1);
            i += 2;
            continue;
        }
        if code != 0 {
            i += 1;
            continue;
        }
        let mut e = Ent {
            kind: value.to_string(),
            ..Ent::default()
        };
        let mut name = String::new();
        i += 1;
        while i < pairs.len() && pairs[i].0 != 0 {
            let (c, v) = pairs[i];
            match c {
                8 => e.layer = v.to_string(),
                1 => e.text = v.to_string(),
                2 => name = v.to_string(),
                // handles and owners are the file's own
                5 | 330 | 340 | 350 | 360 | 390 => {}
                _ => {
                    if let Ok(x) = v.parse::<f64>() {
                        e.nums.push((c, x));
                    }
                }
            }
            i += 1;
        }
        if !matches!(section, "BLOCKS" | "ENTITIES") {
            continue;
        }
        let list = if section == "BLOCKS" {
            blocks.last_mut().map(|b| &mut b.1)
        } else {
            Some(&mut ms)
        };
        match e.kind.as_str() {
            "BLOCK" => blocks.push((name, Vec::new())),
            "ENDBLK" | "SEQEND" => {}
            "VERTEX" => {
                let get = |c| {
                    e.nums
                        .iter()
                        .find(|(k, _)| *k == c)
                        .map_or(0.0, |(_, v)| *v)
                };
                if let Some(last) = list.and_then(|l| l.last_mut()) {
                    last.pts.push((get(10), get(20)));
                }
            }
            _ => {
                if e.kind == "LWPOLYLINE" {
                    let xs: Vec<f64> = e
                        .nums
                        .iter()
                        .filter(|(c, _)| *c == 10)
                        .map(|(_, v)| *v)
                        .collect();
                    let ys: Vec<f64> = e
                        .nums
                        .iter()
                        .filter(|(c, _)| *c == 20)
                        .map(|(_, v)| *v)
                        .collect();
                    e.pts = xs.into_iter().zip(ys).collect();
                    e.nums.retain(|(c, _)| *c != 10 && *c != 20);
                }
                if e.kind == "INSERT" {
                    e.text = name;
                }
                if let Some(l) = list {
                    l.push(e);
                }
            }
        }
    }
    (blocks, ms)
}

fn hausdorff(a: &[(f64, f64)], b: &[(f64, f64)]) -> f64 {
    let v = |p: &[(f64, f64)]| p.iter().map(|&(x, y)| Vec2::new(x, y)).collect::<Vec<_>>();
    let (a, b) = (v(a), v(b));
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
    dir(&a, &b).max(dir(&b, &a))
}

fn same_pts(a: &[(f64, f64)], b: &[(f64, f64)]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(p, q)| (p.0 - q.0).abs() <= 0.011 && (p.1 - q.1).abs() <= 0.011)
}

fn same_nums(a: &Ent, b: &Ent, skip_xy: bool) -> bool {
    let keep = |e: &Ent| -> Vec<(i32, f64)> {
        e.nums
            .iter()
            .copied()
            .filter(|(c, _)| !(skip_xy && matches!(c, 10 | 20 | 11 | 21)))
            .collect()
    };
    let (x, y) = (keep(a), keep(b));
    x.len() == y.len()
        && x.iter()
            .zip(&y)
            .all(|(p, q)| p.0 == q.0 && (p.1 - q.1).abs() <= 0.011)
}

#[test]
fn dxfs_match_the_reference() {
    let fx = fixture("export", "targets");
    let mut failures = Vec::new();
    let (mut entities, mut items) = (0, 0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let ref_dxf = case["output"]["dxf"].as_object().unwrap();
        if ref_dxf.is_empty() {
            continue;
        }
        let opts = ExportOptions {
            created: case["input"]["created"].as_str().map(str::to_string),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let pieces = ctx.piece_reference().unwrap();
        let ref_manifest: Value =
            serde_json::from_str(case["output"]["manifest"].as_str().unwrap()).unwrap();
        let ref_checks = case["output"]["checks"].as_array().unwrap();
        let doc = pat.doc.as_ref().unwrap();
        // which pieces' cut lines come from the union
        let union_blocks: Vec<String> = doc
            .pieces
            .iter()
            .zip(match &pieces {
                ftx_export::PyJson::Dict(items) => {
                    items.iter().map(|(k, _)| k.clone()).collect::<Vec<_>>()
                }
                _ => Vec::new(),
            })
            .filter(|(p, _)| {
                let inst = primary_instance(&pat, &p.id).unwrap();
                cut_outline(inst, &doc.pieces[inst.piece_index], None, ctx.sa, 4.0)
                    .unwrap()
                    .1
                    == Method::Union
            })
            .map(|(_, b)| b)
            .collect();
        for astm in [true, false] {
            let (name, text, _) = export_dxf_astm(&ctx, &pieces, astm).unwrap();
            let want = ref_dxf[&name].as_str().unwrap();
            let ((gb, gm), (wb, wm)) = (parse(&text), parse(want));
            let gnames: Vec<&String> = gb.iter().map(|b| &b.0).collect();
            let wnames: Vec<&String> = wb
                .iter()
                .map(|b| &b.0)
                .filter(|n| !n.starts_with('*') && !n.starts_with('$'))
                .collect();
            if gnames != wnames {
                failures.push(format!("{id}/{name}: blocks {gnames:?} vs {wnames:?}"));
                continue;
            }
            for (bname, gents) in &gb {
                let wents = &wb.iter().find(|b| &b.0 == bname).unwrap().1;
                let union = union_blocks.contains(bname);
                let mut keys: Vec<(String, String)> = gents
                    .iter()
                    .chain(wents.iter())
                    .map(|e| (e.kind.clone(), e.layer.clone()))
                    .collect();
                keys.sort();
                keys.dedup();
                for (kind, layer) in keys {
                    let g: Vec<&Ent> = gents
                        .iter()
                        .filter(|e| e.kind == kind && e.layer == layer)
                        .collect();
                    let w: Vec<&Ent> = wents
                        .iter()
                        .filter(|e| e.kind == kind && e.layer == layer)
                        .collect();
                    let at = format!("{id}/{name}/{bname} {kind} layer {layer}");
                    let turn_points = kind == "POINT" && (layer == "2" || layer == "3");
                    if turn_points && union {
                        continue;
                    }
                    if g.len() != w.len() {
                        failures.push(format!("{at}: {} vs {}", g.len(), w.len()));
                        continue;
                    }
                    for (a, b) in g.iter().zip(&w) {
                        entities += 1;
                        let ok = match kind.as_str() {
                            "POLYLINE" if union && (layer == "1" || layer == "84") => {
                                hausdorff(&a.pts, &b.pts) <= 0.05
                            }
                            "POLYLINE" => same_pts(&a.pts, &b.pts) && same_nums(a, b, false),
                            "TEXT"
                                if layer == "1"
                                    || a.nums.iter().any(|(c, v)| *c == 40 && *v == 5.0) =>
                            {
                                a.text == b.text
                            }
                            _ => a.text == b.text && same_nums(a, b, false),
                        };
                        if !ok {
                            failures.push(format!("{at}:\n  got  {a:?}\n  want {b:?}"));
                        }
                    }
                }
            }
            let gm: Vec<(String, String)> = gm
                .iter()
                .map(|e| (e.kind.clone(), e.text.clone()))
                .collect();
            let wm: Vec<(String, String)> = wm
                .iter()
                .map(|e| (e.kind.clone(), e.text.clone()))
                .collect();
            if gm != wm {
                failures.push(format!(
                    "{id}/{name}: modelspace\n  got  {gm:?}\n  want {wm:?}"
                ));
            }
            // the checks
            let want_items: Vec<&Value> = ref_checks
                .iter()
                .filter(|i| i["file"] == name.as_str())
                .collect();
            let on_ref = check_dxf(want.as_bytes(), &name, &ref_manifest["pieces"], None);
            for (g, w) in on_ref.iter().zip(&want_items) {
                items += 1;
                if g.to_json() != **w {
                    failures.push(format!(
                        "{id}/{name}: check on the reference's file\n  got  {}\n  want {}",
                        g.to_json(),
                        w
                    ));
                }
            }
            if on_ref.len() != want_items.len() {
                failures.push(format!(
                    "{id}/{name}: {} check items vs {}",
                    on_ref.len(),
                    want_items.len()
                ));
            }
            for item in check_dxf(text.as_bytes(), &name, &pieces.to_value(), None) {
                if !item.ok {
                    failures.push(format!("{id}/{name}: ftx's file fails {item:?}"));
                }
            }
        }
        // flat
        let (name, text, _) = export_flat_dxf(&ctx).unwrap();
        let want = ref_dxf[&name].as_str().unwrap();
        let ((_, gm), (_, wm)) = (parse(&text), parse(want));
        if gm.len() != wm.len() {
            failures.push(format!(
                "{id}/{name}: {} entities vs {}",
                gm.len(),
                wm.len()
            ));
        } else {
            // per laid-out piece: how many TEXT polylines its main label has
            let lay = ftx_export::shelf_layout(
                &ctx.instance_geoms(8.0).unwrap(),
                20.0,
                10.0,
                None,
                true,
                false,
                0.0,
            );
            let mut main_counts = lay.items.iter().map(|g| {
                label_texts(&g.labels[0], false, 0.0, 1.0)
                    .iter()
                    .map(|(x, y, t, s, _)| {
                        stroke_text(t, *x, *y, s * CAP, Anchor::Middle, false).len()
                    })
                    .sum::<usize>()
            });
            let (mut in_text, mut left, mut cuts) = (false, 0usize, 0usize);
            for (n, (a, b)) in gm.iter().zip(&wm).enumerate() {
                entities += 1;
                if a.kind == "LWPOLYLINE"
                    && a.layer == "CUT"
                    && a.nums.iter().any(|(c, v)| *c == 70 && *v == 1.0)
                {
                    cuts += 1;
                    in_text = false;
                }
                if a.layer == "TEXT" && !in_text {
                    in_text = true;
                    left = main_counts.next().unwrap_or(0);
                }
                let ok = if a.kind != b.kind || a.layer != b.layer {
                    false
                } else if a.layer == "TEXT" && left > 0 {
                    left -= 1;
                    a.pts.len() == b.pts.len()
                } else if a.layer == "CUT"
                    && a.nums.iter().any(|(c, v)| *c == 70 && *v == 1.0)
                    && !same_pts(&a.pts, &b.pts)
                {
                    hausdorff(&a.pts, &b.pts) <= 0.05
                } else {
                    same_pts(&a.pts, &b.pts) && same_nums(a, b, false)
                };
                if !ok {
                    failures.push(format!(
                        "{id}/{name} entity {n} (cut {cuts}):\n  got  {a:?}\n  want {b:?}"
                    ));
                    break;
                }
            }
        }
        let want_items: Vec<&Value> = ref_checks
            .iter()
            .filter(|i| i["file"] == name.as_str())
            .collect();
        for (g, w) in check_flat_dxf(want, &name).iter().zip(&want_items) {
            items += 1;
            if g.to_json() != **w {
                failures.push(format!(
                    "{id}/{name}: check on the reference's file\n  got  {}\n  want {}",
                    g.to_json(),
                    w
                ));
            }
        }
        for item in check_flat_dxf(&text, &name) {
            if !item.ok {
                failures.push(format!("{id}/{name}: ftx's file fails {item:?}"));
            }
        }
    }
    println!("DXF: {entities} entities compared; checkers reproduce {items} reference items");
    assert!(
        failures.is_empty(),
        "{} failures:\n{}",
        failures.len(),
        failures
            .iter()
            .take(25)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}
