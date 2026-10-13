//! Export everything and check it, end to end, against `tests/fixtures/oracle/export/all.json` (captured by
//! `tools/oracle/capture_exports.py`; spec 03 §1.3, §7.13, §8.1, §10.3): TESTBODICE, ROBUST and the 92 cm bolero
//! with and without soutache. The same files; the same manifest line for line (numbers within 0.05 mm, where
//! union outlines enter); the checks run over ftx's files in the reference's order with the same names, all
//! passing, as many as the reference's (the spec's goldens: TESTBODICE 146 and with the pattern 150, ROBUST 125,
//! the braided bolero 183 with the pattern).

use std::collections::BTreeMap;

use ftx_conformance::fixture;
use ftx_export::{Ctx, ExportOptions, check_files, export_all};
use ftx_pattern::Pattern;
use serde_json::Value;

/// Numbers and the text between them.
fn tokens(line: &str) -> (Vec<String>, Vec<f64>) {
    let (mut text, mut nums, mut cur, mut num) =
        (Vec::new(), Vec::new(), String::new(), String::new());
    let flush =
        |num: &mut String, nums: &mut Vec<f64>, text: &mut Vec<String>, cur: &mut String| {
            if !num.is_empty() {
                match num.parse::<f64>() {
                    Ok(v) => {
                        text.push(std::mem::take(cur));
                        nums.push(v);
                    }
                    Err(_) => cur.push_str(num),
                }
                num.clear();
            }
        };
    for c in line.chars() {
        if c.is_ascii_digit()
            || c == '.'
            || c == 'e' && !num.is_empty()
            || (c == '-' && num.is_empty())
        {
            num.push(c);
        } else {
            flush(&mut num, &mut nums, &mut text, &mut cur);
            cur.push(c);
        }
    }
    flush(&mut num, &mut nums, &mut text, &mut cur);
    text.push(cur);
    (text, nums)
}

#[test]
fn export_all_and_check_match_the_reference() {
    let fx = fixture("export", "all");
    let mut failures = Vec::new();
    let mut summary = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let out = &case["output"];
        let opts = ExportOptions {
            created: case["input"]["created"].as_str().map(str::to_string),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let set = export_all(&ctx).unwrap();
        let files: BTreeMap<String, Vec<u8>> = set.files.iter().cloned().collect();
        let names: Vec<&String> = files.keys().collect();
        let want: Vec<&str> = out["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        if names.iter().map(|s| s.as_str()).collect::<Vec<_>>() != want {
            failures.push(format!("{id}: files\n  got  {names:?}\n  want {want:?}"));
        }
        // the manifest, line for line
        let ours = String::from_utf8(
            files
                .iter()
                .find(|(k, _)| k.ends_with("_export_manifest.json"))
                .unwrap()
                .1
                .clone(),
        )
        .unwrap();
        let theirs = out["manifest"].as_str().unwrap();
        let (g, w): (Vec<&str>, Vec<&str>) = (ours.lines().collect(), theirs.lines().collect());
        if g.len() != w.len() {
            failures.push(format!("{id}: manifest {} lines vs {}", g.len(), w.len()));
        } else {
            for (n, (a, b)) in g.iter().zip(&w).enumerate() {
                if a == b {
                    continue;
                }
                let ((at, an), (bt, bn)) = (tokens(a), tokens(b));
                if at != bt
                    || an.len() != bn.len()
                    || an.iter().zip(&bn).any(|(x, y)| (x - y).abs() > 0.05)
                {
                    failures.push(format!(
                        "{id}: manifest line {}\n  got  {a}\n  want {b}",
                        n + 1
                    ));
                }
            }
        }
        // the checks
        for (key, with) in [("checks", None), ("checks_with_pattern", Some(&pat))] {
            let items = check_files(&files, "out", with);
            let names: Vec<&str> = items.iter().map(|i| i.check.as_str()).collect();
            let want: Vec<&str> = out[key]
                .as_array()
                .unwrap()
                .iter()
                .map(|i| i["check"].as_str().unwrap())
                .collect();
            if names != want {
                failures.push(format!(
                    "{id}: {key} names differ ({} vs {})",
                    names.len(),
                    want.len()
                ));
            }
            for i in items.iter().filter(|i| !i.ok) {
                failures.push(format!("{id}: {key}: {i:?}"));
            }
            let ref_ok = out[key]
                .as_array()
                .unwrap()
                .iter()
                .filter(|i| i["ok"] == Value::Bool(true))
                .count();
            summary.push(format!(
                "{id} {key} {}/{} (reference {ref_ok}/{})",
                items.iter().filter(|i| i.ok).count(),
                items.len(),
                want.len()
            ));
        }
    }
    println!("export all + check:\n  {}", summary.join("\n  "));
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
fn a_missing_manifest_or_file_is_a_failed_item() {
    let fx = fixture("export", "all");
    let case = &fx["cases"][0];
    let pat = Pattern::new(case["input"]["pattern"].clone());
    let ctx = Ctx::new(
        &pat,
        ExportOptions {
            created: Some("2026-10-11T12:00".into()),
            ..ExportOptions::default()
        },
    )
    .unwrap();
    let set = export_all(&ctx).unwrap();
    let mut files: BTreeMap<String, Vec<u8>> = set.files.iter().cloned().collect();
    let empty = BTreeMap::new();
    let items = check_files(&empty, "out", None);
    assert_eq!(
        (items.len(), items[0].check.as_str(), items[0].ok),
        (1, "manifest", false)
    );
    files.remove("TESTBODICE_plot.plt");
    let items = check_files(&files, "out", None);
    assert!(
        items
            .iter()
            .any(|i| i.check == "file.missing" && i.file == "TESTBODICE_plot.plt" && !i.ok)
    );
}
