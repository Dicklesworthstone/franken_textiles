//! Properties of `annotate` and its reference-order layout (`annotate_py`) on the traced drafts of the
//! tracing oracle (`trace/post.json`), with generated names.json: edge renames, drawn-side mirroring, cut
//! quantities and names. The oracle pins the reference's bytes on 13 documents; here, for any such names,
//! the laid-out file re-reads as exactly the typed result's data, and mirroring a piece twice gives every
//! vertex back to within the 0.001 mm rounding.

use ftx_conformance::fixture_dir;
use ftx_conformance::propcheck::{Stream, check};
use ftx_export::pyjson::PyJson;
use ftx_pattern::doc::PatternDoc;
use ftx_trace::{OValue, annotate, annotate_py};
use serde_json::Value;

/// The traced drafts, in their key order and typed (parsed from the same text).
fn drafts() -> Vec<(PyJson, PatternDoc)> {
    let raw = std::fs::read_to_string(fixture_dir().join("trace/post.json")).unwrap();
    let fx = PyJson::parse(&raw).unwrap();
    ["plain", "navy", "filled"]
        .iter()
        .map(|name| {
            let d = fx.get("drafts").and_then(|d| d.get(name)).unwrap().clone();
            let typed = serde_json::from_str(&d.dumps(Some(1))).unwrap();
            (d, typed)
        })
        .collect()
}

/// names.json for one piece: bit 0 mirrors it, bit 1 renames its first edge, bit 2 cuts two, bit 3 names it.
fn names(doc: &PatternDoc, piece: u64, flags: u64) -> (String, OValue) {
    let p = &doc.pieces[(piece % doc.pieces.len() as u64) as usize];
    let mut spec = serde_json::Map::new();
    if flags & 1 != 0 {
        spec.insert("drawn_side".into(), "L".into());
    }
    if flags & 2 != 0 {
        let mut edges = serde_json::Map::new();
        edges.insert(p.edges[0].id.clone(), "renamed".into());
        spec.insert("edges".into(), Value::Object(edges));
    }
    if flags & 4 != 0 {
        spec.insert("cut".into(), serde_json::json!({"qty": 2}));
    }
    if flags & 8 != 0 {
        spec.insert("name".into(), "A piece".into());
    }
    let text =
        serde_json::json!({"garment": "generated", "pieces": {p.id.clone(): spec}}).to_string();
    (p.id.clone(), serde_json::from_str(&text).unwrap())
}

/// JSON data equality with numbers compared as numbers.
fn same_data(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.as_f64() == y.as_f64(),
        (Value::Array(x), Value::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same_data(p, q))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len()
                && x.iter()
                    .all(|(k, v)| y.get(k).is_some_and(|w| same_data(v, w)))
        }
        _ => a == b,
    }
}

#[test]
fn the_reference_order_layout_keeps_the_typed_data() {
    let ds = drafts();
    check(
        "annotate: annotate_py re-reads as the typed result",
        0xA2207,
        200,
        |s: &mut Stream| (s.next_u64(), (s.next_u64(), s.next_u64() % 16)),
        |&(d, (piece, flags))| {
            let (ordered, typed) = &ds[(d % ds.len() as u64) as usize];
            let (_, n) = names(typed, piece, flags);
            let Ok((laid_out, data)) = annotate_py(ordered, typed, &n) else {
                return false;
            };
            let reread: Value = serde_json::from_str(&laid_out.dumps(Some(1))).unwrap();
            same_data(&reread, &serde_json::to_value(&data).unwrap())
        },
    );
}

#[test]
fn mirroring_a_piece_twice_gives_it_back() {
    let ds = drafts();
    check(
        "annotate: drawn_side L twice",
        0x3112,
        100,
        |s: &mut Stream| (s.next_u64(), s.next_u64()),
        |&(d, piece)| {
            let (_, typed) = &ds[(d % ds.len() as u64) as usize];
            let (pid, n) = names(typed, piece, 1);
            let Ok(once) = annotate(typed, &n) else {
                return false;
            };
            let Ok(twice) = annotate(&once, &n) else {
                return false;
            };
            let find = |doc: &PatternDoc| doc.pieces.iter().find(|p| p.id == pid).cloned();
            let (Some(a), Some(b)) = (find(typed), find(&twice)) else {
                return false;
            };
            a.edges
                .iter()
                .map(|e| (&e.id, &e.from, &e.to))
                .eq(b.edges.iter().map(|e| (&e.id, &e.from, &e.to)))
                && a.vertices
                    .iter()
                    .zip(b.vertices.iter())
                    .all(|((ka, pa), (kb, pb))| {
                        ka == kb
                            && (pa.x - pb.x).abs() <= 1.0e-3 + 1.0e-9
                            && (pa.y - pb.y).abs() <= 1.0e-3 + 1.0e-9
                    })
        },
    );
}
