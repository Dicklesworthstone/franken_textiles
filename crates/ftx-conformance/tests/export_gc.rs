//! GarmentCode JSON against the reference's files in `tests/fixtures/oracle/export/targets.json` (spec 03 §7.11,
//! §8, §10.4): TESTBODICE, ROBUST and the 92 cm fez, bolero and trousers. The same panels, edges, curvature types,
//! stitches, panel order, edge map, warnings and properties; vertices within 1e-4 cm and curvature parameters
//! within 1e-5 (most files come out byte-identical); the same manifest entry; ftx's GarmentCode checks give
//! exactly the reference checker's items on its files and pass on ftx's.

use ftx_conformance::fixture;
use ftx_export::{Ctx, ExportOptions, check_garmentcode, export_garmentcode};
use ftx_pattern::Pattern;
use ftx_pattern::garmentcode_import::import_garmentcode;
use serde_json::{Value, json};

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

fn rectangle(id: &str, width: f64, qty: usize) -> Value {
    json!({
        "id": id, "cut": {"qty": qty},
        "vertices": {"a": [0, 0], "b": [width, 0], "c": [width, 60], "d": [0, 60]},
        "edges": [
            {"id": "bottom", "from": "a", "to": "b"},
            {"id": "right", "from": "b", "to": "c"},
            {"id": "top", "from": "c", "to": "d"},
            {"id": "left", "from": "d", "to": "a"}
        ]
    })
}

fn exported(pattern: &Pattern) -> (String, Value) {
    assert!(pattern.structure_errors().is_empty());
    let ctx = Ctx::new(pattern, ExportOptions::default()).unwrap();
    let (name, text, entry) = export_garmentcode(&ctx).unwrap();
    assert!(check_garmentcode(&text, &name).iter().all(|item| item.ok));
    let value: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(
        value["pattern"]["panels"].as_object().unwrap().len(),
        entry.to_value()["panels"].as_u64().unwrap() as usize,
        "every reported physical panel must survive JSON parsing"
    );
    (text, value)
}

#[test]
fn colliding_panel_names_preserve_geometry_and_stitches_on_reimport() {
    let mut raw = json!({
        "schema": "fabric-pattern/1", "units": "mm",
        "pieces": [rectangle("front", 30.0, 2), rectangle("front_R", 70.0, 1),
                   rectangle("front_R_2", 45.0, 1)],
        "seams": [
            {"id": "right", "a": "front.R/right", "b": "front_R/left"},
            {"id": "left", "a": "front.L/right", "b": "front_R_2/left", "reverse": false}
        ]
    });
    // A generated suffix must not steal a later panel's ordinary name. Reordering the source
    // changes which collision keeps its base name, but never the source geometry or connectivity.
    for reversed in [false, true] {
        if reversed {
            raw["pieces"].as_array_mut().unwrap().reverse();
        }
        let pattern = Pattern::new(raw.clone());
        let (text, output) = exported(&pattern);
        assert_eq!(text, exported(&pattern).0);
        let panels = output["pattern"]["panels"].as_object().unwrap();
        assert_eq!(panels.len(), 4);
        let instance_map = output["fabric_pattern_export"]["instance_map"]
            .as_object()
            .unwrap();
        assert_eq!(instance_map.len(), 4);
        assert_eq!(instance_map["front_R_2"], "front_R_2");
        for (name, instance_id) in instance_map {
            let instance = pattern.instance(instance_id.as_str().unwrap()).unwrap();
            let expected: Vec<Value> = instance
                .edges
                .iter()
                .map(|edge| {
                    let p = instance.verts.get(&edge.from).unwrap();
                    json!([p.x / 10.0, p.y / 10.0])
                })
                .collect();
            assert_eq!(panels[name]["vertices"], json!(expected));
        }
        let stitches = output["pattern"]["stitches"].as_array().unwrap();
        assert_eq!(stitches.len(), 2);
        for (stitch, expected) in stitches.iter().zip([
            [("front.R", "right"), ("front_R", "left")],
            [("front.L", "right"), ("front_R_2", "left")],
        ]) {
            for (side, (instance, edge)) in stitch.as_array().unwrap()[..2].iter().zip(expected) {
                let name = side["panel"].as_str().unwrap();
                assert_eq!(instance_map[name], instance);
                assert_eq!(
                    output["fabric_pattern_export"]["edge_map"][name]
                        [side["edge"].as_u64().unwrap() as usize],
                    edge
                );
            }
        }
        let imported = import_garmentcode(&output, None).unwrap();
        let roundtrip = Pattern::new(imported.pattern);
        assert!(roundtrip.structure_errors().is_empty());
        assert_eq!(roundtrip.instances.len(), 4);
        assert_eq!(roundtrip.seams().len(), 2);
        for (seam, reverse) in roundtrip.seams().iter().zip([true, false]) {
            assert_eq!(seam.reverse(), reverse);
            for side in ["a", "b"] {
                assert_eq!(
                    roundtrip.side_length(seam.side_ref(side).unwrap()).unwrap(),
                    60.0
                );
            }
        }
    }
}

#[test]
fn exported_seam_direction_matches_the_canonical_pattern() {
    for reverse in [
        None,
        Some(json!(true)),
        Some(json!(false)),
        Some(Value::Null),
        Some(json!(0)),
        Some(json!("")),
        Some(json!([])),
        Some(json!({})),
        Some(json!("explicit truthy value")),
    ] {
        let mut raw = json!({
            "schema": "fabric-pattern/1", "units": "mm",
            "pieces": [rectangle("a", 30.0, 1), rectangle("b", 70.0, 1)],
            "seams": [{"id": "s", "a": "a/right", "b": "b/left"}]
        });
        if let Some(value) = reverse {
            raw["seams"][0]["reverse"] = value;
        }
        let pattern = Pattern::new(raw);
        let (_, output) = exported(&pattern);
        let expected = pattern.seams()[0].reverse();
        let stitch = output["pattern"]["stitches"][0].as_array().unwrap();
        assert_eq!(stitch.len(), if expected { 2 } else { 3 });
        if !expected {
            assert_eq!(stitch[2], "right_wrong");
        }
        let imported = import_garmentcode(&output, None).unwrap();
        assert_eq!(
            Pattern::new(imported.pattern).seams()[0].reverse(),
            expected
        );
    }
}
