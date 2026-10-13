//! `ftx-pattern` against the oracle fixtures captured by `tools/oracle/capture_pattern_geometry.py`
//! (`instances`) and `tools/oracle/capture_pattern_model.py` (`structure`, `seams`, `drafts`):
//! instances, structure-check messages, seam sides, notch fractions, seam pairs, tangents, corner
//! angles, and the semantic round trip of drafted garments.

use ftx_conformance::{Compare, fixture};
use ftx_core::{End, Vec2};
use ftx_pattern::{Pattern, Seam};
use serde_json::{Value, json};

fn v2(p: Vec2) -> Value {
    json!([p.x, p.y])
}

/// Structure errors compare exactly, except check-1 messages (the reference walks vertex ids in Python
/// set order), which compare as a multiset.
fn same_errors(got: &[String], want: &Value) -> bool {
    let want: Vec<String> = want
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_string())
        .collect();
    let is_c1 = |m: &String| m.contains("must not contain '.', '/', '+' or spaces");
    let (mut g1, g2): (Vec<_>, Vec<_>) = got.iter().cloned().partition(is_c1);
    let (mut w1, w2): (Vec<_>, Vec<_>) = want.iter().cloned().partition(is_c1);
    g1.sort();
    w1.sort();
    g1 == w1 && g2 == w2
}

fn describe_side(p: &Pattern, r: &str) -> Value {
    let run = || -> Result<Value, ftx_pattern::PyError> {
        let (pts, junctions) = p.side_polyline(r)?;
        Ok(json!({
            "length": p.side_length(r)?,
            "junctions": junctions,
            "points": pts.len(),
            "notch_fractions": p.notch_fractions(r)?,
        }))
    };
    run().unwrap_or_else(|e| json!({"error": e.to_string()}))
}

fn describe_pairs(p: &Pattern, s: &Seam, spacing: f64) -> Value {
    match p.seam_pairs(s, spacing) {
        Ok(r) => {
            let mut v = serde_json::to_value(&r).unwrap();
            v["a"] = Value::Array(r.a.iter().map(|&q| v2(q)).collect());
            v["b"] = Value::Array(r.b.iter().map(|&q| v2(q)).collect());
            v
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn describe_instances(p: &Pattern) -> Value {
    Value::Array(
        p.instances
            .iter()
            .map(|inst| {
                let mut lengths = serde_json::Map::new();
                let mut tangents = serde_json::Map::new();
                let mut corners = serde_json::Map::new();
                for e in &inst.edges {
                    lengths.insert(e.id.clone(), json!(inst.edge_length(&e.id).unwrap()));
                    let (s, t) = (inst.edge_tangent(&e.id, End::Start).unwrap(), inst.edge_tangent(&e.id, End::End).unwrap());
                    tangents.insert(e.id.clone(), json!([v2(s), v2(t)]));
                    corners.insert(e.to.clone(), json!(inst.corner_angle(&e.to).unwrap()));
                }
                json!({
                    "id": inst.id, "piece": inst.piece, "side": inst.side.as_str(), "mirrored": inst.mirrored,
                    "area": inst.area().unwrap(), "perimeter": inst.perimeter().unwrap(),
                    "edge_lengths": lengths, "tangents": tangents, "corner_angles": corners,
                })
            })
            .collect(),
    )
}

#[test]
fn instances_match_the_reference() {
    let fx = fixture("pattern", "instances");
    let mut cmp = Compare::new(1e-9);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let p = Pattern::new(case["input"].clone());
        let out = &case["output"];
        cmp.json(
            &format!("{id} structure_errors"),
            &json!(p.structure_errors()),
            &out["structure_errors"],
        );
        let got: Vec<Value> = p
            .instances
            .iter()
            .map(|inst| {
                let verts: serde_json::Map<String, Value> = inst.verts.iter().map(|(k, q)| (k.to_string(), v2(q))).collect();
                let edges: Vec<Value> = inst
                    .edges
                    .iter()
                    .map(|e| json!({"id": e.id, "from": e.from, "to": e.to, "curve": serde_json::to_value(&e.curve).unwrap()}))
                    .collect();
                let lengths: serde_json::Map<String, Value> =
                    inst.edges.iter().map(|e| (e.id.clone(), json!(inst.edge_length(&e.id).unwrap()))).collect();
                json!({
                    "id": inst.id, "piece": inst.piece, "side": inst.side.as_str(), "mirrored": inst.mirrored,
                    "verts": verts, "edges": edges, "area": inst.area().unwrap(), "perimeter": inst.perimeter().unwrap(),
                    "edge_lengths": lengths,
                })
            })
            .collect();
        cmp.json(id, &Value::Array(got), &out["instances"]);
    }
    cmp.finish("pattern instances");
}

#[test]
fn structure_messages_match_the_reference() {
    let fx = fixture("pattern", "structure");
    let mut failures = Vec::new();
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let p = Pattern::new(case["input"].clone());
        let errs = p.structure_errors();
        if !same_errors(&errs, &case["output"]["structure_errors"]) {
            failures.push(format!(
                "{id}: got {errs:?}\n      want {}",
                case["output"]["structure_errors"]
            ));
        }
        let ids: Vec<&str> = p.instances.iter().map(|i| i.id.as_str()).collect();
        if json!(ids) != case["output"]["instances"] {
            failures.push(format!(
                "{id}: instances {ids:?}, want {}",
                case["output"]["instances"]
            ));
        }
    }
    println!(
        "pattern structure: {} cases",
        fx["cases"].as_array().unwrap().len()
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

fn compare_model_case(cmp: &mut Compare, id: &str, p: &Pattern, out: &Value, with_spacing_7: bool) {
    cmp.json(
        &format!("{id} structure_errors"),
        &json!(p.structure_errors()),
        &out["structure_errors"],
    );
    if p.build_errors.is_empty() {
        cmp.json(
            &format!("{id} instances"),
            &describe_instances(p),
            &out["instances"],
        );
    }
    let seams = p.seams();
    let want = out["seams"].as_array().unwrap();
    assert_eq!(seams.len(), want.len(), "{id}: seam count");
    for (s, w) in seams.iter().zip(want) {
        let sid = format!("{id} seam {}", s.id_text());
        cmp.json(
            &format!("{sid} a"),
            &describe_side(p, s.side_ref("a").unwrap()),
            &w["a"],
        );
        cmp.json(
            &format!("{sid} b"),
            &describe_side(p, s.side_ref("b").unwrap()),
            &w["b"],
        );
        cmp.json(
            &format!("{sid} pairs"),
            &describe_pairs(p, s, 5.0),
            &w["pairs"],
        );
        if with_spacing_7 {
            cmp.json(
                &format!("{sid} pairs@7"),
                &describe_pairs(p, s, 7.0),
                &w["pairs_spacing_7"],
            );
        }
    }
}

#[test]
fn seams_tangents_and_corners_match_the_reference() {
    let fx = fixture("pattern", "seams");
    let mut cmp = Compare::new(1e-9);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        compare_model_case(
            &mut cmp,
            id,
            &Pattern::new(case["input"].clone()),
            &case["output"],
            true,
        );
    }
    cmp.finish("pattern seams");
}

#[test]
fn drafted_garments_match_and_round_trip() {
    let fx = fixture("pattern", "drafts");
    let mut cmp = Compare::new(1e-9);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let pattern = &case["input"]["pattern"];
        let p = Pattern::from_json(&pattern.to_string()).unwrap();
        compare_model_case(&mut cmp, id, &p, &case["output"], false);
        cmp.json(&format!("{id} round trip"), &p.to_value().unwrap(), pattern);
    }
    cmp.finish("drafted garments");
}
