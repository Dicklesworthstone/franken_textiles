//! GarmentCode JSON against the reference's files in `tests/fixtures/oracle/export/targets.json` (spec 03 §7.11,
//! §8, §10.4): TESTBODICE, ROBUST and the 92 cm fez, bolero and trousers. The same panels, edges, curvature types,
//! stitches, panel order, edge map, warnings and properties; vertices within 1e-4 cm and curvature parameters
//! within 1e-5 (most files come out byte-identical); the same manifest entry; ftx's GarmentCode checks give
//! exactly the reference checker's items on its files and pass on ftx's.

use ftx_conformance::fixture;
use ftx_export::{Ctx, ExportOptions, check_garmentcode, export_garmentcode};
use ftx_pattern::Pattern;
use serde_json::Value;

/// Equal, numbers within `tol` (`tol_of(path)` picks the tolerance by where the number sits).
fn close(a: &Value, b: &Value, path: &str, failures: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let tol = if path.contains("params") { 1e-5 } else { 1e-4 };
            if (x.as_f64().unwrap() - y.as_f64().unwrap()).abs() > tol {
                failures.push(format!("{path}: {x} vs {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                close(p, q, &format!("{path}[{i}]"), failures);
            }
        }
        (Value::Object(x), Value::Object(y)) if x.len() == y.len() => {
            for (k, q) in y {
                match x.get(k) {
                    Some(p) => close(p, q, &format!("{path}.{k}"), failures),
                    None => failures.push(format!("{path}: missing {k}")),
                }
            }
        }
        _ if a == b => {}
        _ => failures.push(format!("{path}: {a} vs {b}")),
    }
}

#[test]
fn garmentcode_matches_the_reference() {
    let fx = fixture("export", "targets");
    let mut failures = Vec::new();
    let (mut files, mut identical, mut items) = (0, 0, 0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let want = case["output"]["garmentcode"].as_str().unwrap();
        if want.is_empty() {
            continue;
        }
        files += 1;
        let opts = ExportOptions {
            created: case["input"]["created"].as_str().map(str::to_string),
            ..ExportOptions::default()
        };
        let pat = Pattern::new(case["input"]["pattern"].clone());
        let ctx = Ctx::new(&pat, opts).unwrap();
        let (name, text, entry) = export_garmentcode(&ctx).unwrap();
        if text == want {
            identical += 1;
        } else {
            let (g, w): (Value, Value) = (
                serde_json::from_str(&text).unwrap(),
                serde_json::from_str(want).unwrap(),
            );
            close(&g, &w, id, &mut failures);
        }
        let manifest: Value =
            serde_json::from_str(case["output"]["manifest"].as_str().unwrap()).unwrap();
        let ref_items: Vec<&Value> = case["output"]["checks"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|i| i["file"] == name.as_str())
            .collect();
        let on_ref = check_garmentcode(want, &name);
        for (g, w) in on_ref.iter().zip(&ref_items) {
            items += 1;
            if g.to_json() != **w {
                failures.push(format!(
                    "{id}: check on the reference's file\n  got  {}\n  want {}",
                    g.to_json(),
                    w
                ));
            }
        }
        if on_ref.len() != ref_items.len() {
            failures.push(format!(
                "{id}: {} check items vs {}",
                on_ref.len(),
                ref_items.len()
            ));
        }
        for item in check_garmentcode(&text, &name) {
            if !item.ok {
                failures.push(format!("{id}: ftx's file fails {item:?}"));
            }
        }
        if entry.to_value() != manifest["files"]["garmentcode"] {
            failures.push(format!(
                "{id}: manifest {} vs {}",
                entry.to_value(),
                manifest["files"]["garmentcode"]
            ));
        }
    }
    println!(
        "GarmentCode: {files} files, {identical} byte-identical, the rest within tolerance; checker reproduces {items} items"
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
