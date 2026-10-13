//! Soutache DSTs and their README against the reference's in `tests/fixtures/oracle/export/targets.json` (spec 03
//! §7.12, §8, §10.4): TESTBODICE (the fronts carry soutache) and the braided 92 cm bolero. The same files and
//! README; DST bytes identical, or the same record count with every decoded position within one unit (0.1 mm);
//! the same manifest entries; ftx's DST checks give exactly the reference checker's items on its files (also with
//! the pattern's braid lines) and pass on ftx's.

use ftx_conformance::fixture;
use ftx_export::{Ctx, ExportOptions, Source, build_geom, check_dst, export_embroidery, read_dst};
use ftx_pattern::Pattern;
use serde_json::Value;

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn positions(data: &[u8]) -> Vec<(i64, i64)> {
    let (mut x, mut y) = (0, 0);
    read_dst(data)
        .moves
        .iter()
        .map(|&(dx, dy, _)| {
            x += dx;
            y += dy;
            (x, y)
        })
        .collect()
}

#[test]
fn dsts_match_the_reference() {
    let fx = fixture("export", "targets");
    let mut failures = Vec::new();
    let (mut files, mut identical, mut items) = (0, 0, 0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let want = case["output"]["dst"].as_object().unwrap();
        if want.is_empty() {
            continue;
        }
        let opts = ExportOptions {
            created: case["input"]["created"].as_str().map(str::to_string),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let (dsts, readme) = export_embroidery(&ctx).unwrap();
        let names: Vec<&str> = dsts.iter().map(|d| d.name.as_str()).collect();
        let want_names: Vec<&str> = want.keys().map(String::as_str).collect();
        let mut sorted = names.clone();
        sorted.sort_unstable();
        let mut want_sorted = want_names.clone();
        want_sorted.sort_unstable();
        if sorted != want_sorted {
            failures.push(format!("{id}: files {names:?} vs {want_names:?}"));
            continue;
        }
        match readme {
            Some((_, text)) if text == case["output"]["dst_readme"].as_str().unwrap() => {}
            other => failures.push(format!(
                "{id}: README\n  got  {other:?}\n  want {}",
                case["output"]["dst_readme"]
            )),
        }
        let manifest: Value =
            serde_json::from_str(case["output"]["manifest"].as_str().unwrap()).unwrap();
        let checks = case["output"]["checks"].as_array().unwrap();
        let with_pattern = case["output"]["dst_checks_with_pattern"]
            .as_array()
            .unwrap();
        for d in &dsts {
            files += 1;
            let reference = unhex(want[&d.name].as_str().unwrap());
            if d.bytes == reference {
                identical += 1;
            } else {
                let (g, w) = (positions(&d.bytes), positions(&reference));
                if g.len() != w.len()
                    || g.iter()
                        .zip(&w)
                        .any(|(a, b)| (a.0 - b.0).abs() > 1 || (a.1 - b.1).abs() > 1)
                {
                    failures.push(format!(
                        "{id}/{}: {} records vs {}",
                        d.name,
                        g.len(),
                        w.len()
                    ));
                }
            }
            let info = &manifest["files"][&d.key];
            if d.entry.to_value() != *info {
                failures.push(format!(
                    "{id}/{}: manifest\n  got  {}\n  want {info}",
                    d.name,
                    d.entry.to_value()
                ));
            }
            // braid lines of the instance, as the reference's checker takes them from the pattern
            let iid = info["instance"].as_str().unwrap();
            let inst = pat.instances.iter().find(|i| i.id == iid).unwrap();
            let g = build_geom(&pat, Source::Instance(inst), ctx.sa, 4.0, 6.0).unwrap();
            let braid: Vec<Vec<(f64, f64)>> = g
                .decor
                .iter()
                .filter(|dc| dc.kind == "soutache")
                .map(|dc| dc.pts.iter().map(|p| (p.x, p.y)).collect())
                .collect();
            for (got, want_items) in [
                (
                    check_dst(&reference, &d.name, info, None),
                    checks
                        .iter()
                        .filter(|i| i["file"] == d.name.as_str())
                        .collect::<Vec<_>>(),
                ),
                (
                    check_dst(&reference, &d.name, info, Some(&braid)),
                    with_pattern
                        .iter()
                        .filter(|i| i["file"] == d.name.as_str())
                        .collect(),
                ),
            ] {
                if got.len() != want_items.len() {
                    failures.push(format!(
                        "{id}/{}: {} items vs {}",
                        d.name,
                        got.len(),
                        want_items.len()
                    ));
                }
                for (a, b) in got.iter().zip(&want_items) {
                    items += 1;
                    if a.to_json() != **b {
                        failures.push(format!(
                            "{id}/{}: check on the reference's file\n  got  {}\n  want {b}",
                            d.name,
                            a.to_json()
                        ));
                    }
                }
            }
            for item in check_dst(&d.bytes, &d.name, &d.entry.to_value(), Some(&braid)) {
                if !item.ok {
                    failures.push(format!("{id}/{}: ftx's file fails {item:?}", d.name));
                }
            }
        }
    }
    println!(
        "DST: {files} files, {identical} byte-identical; checker reproduces {items} reference items"
    );
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

#[test]
fn a_shifted_braid_is_caught() {
    // spec 03 §10.1 #26: the braid 4 mm off (+x) on front.R fails dst.on_braid_lines; the lengths still pass
    let fx = fixture("export", "targets");
    let case = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "bolero920")
        .unwrap();
    let pat = Pattern::new(case["input"]["pattern"].clone());
    let ctx = Ctx::new(&pat, ExportOptions::default()).unwrap();
    let (dsts, _) = export_embroidery(&ctx).unwrap();
    let d = dsts.iter().find(|d| d.key == "dst_front.R").unwrap();
    let inst = pat.instances.iter().find(|i| i.id == "front.R").unwrap();
    let g = build_geom(&pat, Source::Instance(inst), ctx.sa, 4.0, 6.0).unwrap();
    let shifted: Vec<Vec<(f64, f64)>> = g
        .decor
        .iter()
        .filter(|dc| dc.kind == "soutache")
        .map(|dc| dc.pts.iter().map(|p| (p.x + 4.0, p.y)).collect())
        .collect();
    let items = check_dst(&d.bytes, &d.name, &d.entry.to_value(), Some(&shifted));
    let ok = |name: &str| items.iter().find(|i| i.check == name).unwrap().ok;
    assert!(!ok("dst.on_braid_lines"));
    assert!(ok("dst.couch_length"));
}
