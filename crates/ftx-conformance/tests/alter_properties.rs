//! Metamorphic properties of the alteration ops (spec 01 §10), on the drafted bolero and trousers of the
//! alteration oracle (`alter/alterations.json`), with `ftx_conformance::propcheck` (shrinking, replayable
//! seeds). A move followed by the opposite move, and a slash-spread followed by the opposite spread at the
//! same line, give every point back to within the ops' 0.001 mm rounding. Neither changes the pattern's
//! structure, and each appends exactly one history entry. `alter_py` lays every result out with exactly
//! the typed result's data, so the reference's key order costs nothing. The oracle pins the reference's
//! exact outputs; these relations hold for inputs no fixture contains.

use ftx_conformance::propcheck::{Stream, check};
use ftx_conformance::{fixture, fixture_dir};
use ftx_export::pyjson::PyJson;
use ftx_pattern::Pattern;
use ftx_pattern::alter::{move_vertex, slash_spread};
use ftx_trace::alter_py;
use serde_json::Value;

/// The drafted input patterns of the oracle's successful cases, typed and in their key order.
fn patterns() -> Vec<(Value, PyJson)> {
    let raw = std::fs::read_to_string(fixture_dir().join("alter/alterations.json")).unwrap();
    let ordered = PyJson::parse(&raw).unwrap();
    let Some(PyJson::List(cases)) = ordered.get("cases") else {
        panic!("no cases")
    };
    let fx = fixture("alter", "alterations");
    cases
        .iter()
        .zip(fx["cases"].as_array().unwrap())
        .filter(|(_, typed)| typed["output"].get("doc").is_some())
        .map(|(case, typed)| {
            let ordered = case.get("input").and_then(|i| i.get("pattern")).unwrap();
            (typed["input"]["pattern"].clone(), ordered.clone())
        })
        .collect()
}

/// A piece id and one of its vertex ids, chosen by index (wrapping).
fn pick(pattern: &Value, piece: u64, vertex: u64) -> (String, String) {
    let pieces = pattern["pieces"].as_array().unwrap();
    let p = &pieces[(piece % pieces.len() as u64) as usize];
    let ids: Vec<&String> = p["vertices"].as_object().unwrap().keys().collect();
    (
        p["id"].as_str().unwrap().to_string(),
        ids[(vertex % ids.len() as u64) as usize].clone(),
    )
}

/// Every point of the pattern (vertices, grain, internal/decor/trace-mark points), in a fixed order.
fn points(pattern: &Value) -> Vec<f64> {
    let mut out = Vec::new();
    fn push(out: &mut Vec<f64>, v: &Value) {
        for q in v.as_array().into_iter().flatten() {
            out.extend(q.as_array().into_iter().flatten().filter_map(Value::as_f64));
        }
    }
    for p in pattern["pieces"].as_array().unwrap() {
        let vs = p["vertices"].as_object().unwrap();
        let mut ids: Vec<&String> = vs.keys().collect();
        ids.sort();
        for id in ids {
            out.extend(vs[id].as_array().unwrap().iter().filter_map(Value::as_f64));
        }
        push(&mut out, &p["grain"]);
        for key in ["internal", "decor", "trace_marks"] {
            for item in p[key].as_array().into_iter().flatten() {
                push(&mut out, &item["pts"]);
            }
        }
    }
    out
}

fn history_len(pattern: &Value) -> usize {
    pattern["meta"]["history"].as_array().map_or(0, Vec::len)
}

/// JSON data equality with numbers compared as numbers (an integer 2 is the float 2.0).
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

/// Round trip within the ops' rounding to 0.001 mm, applied twice.
fn round_trip(before: &Value, after: &Value) -> bool {
    let (p, q) = (points(before), points(after));
    p.len() == q.len()
        && p.iter()
            .zip(&q)
            .all(|(x, y)| (x - y).abs() <= 1.0e-3 + 1.0e-9)
}

#[test]
fn a_move_and_its_inverse_give_the_pattern_back() {
    let pats = patterns();
    check(
        "alter: move then the opposite move",
        0xA17E,
        300,
        |s: &mut Stream| {
            (
                s.next_u64(),
                (s.next_u64(), s.next_u64()),
                (s.f64_in(-30.0, 30.0), s.f64_in(-30.0, 30.0), 0.0),
            )
        },
        |&(pat, (piece, vertex), (dx, dy, _))| {
            let (p, ordered) = &pats[(pat % pats.len() as u64) as usize];
            let (pid, vid) = pick(p, piece, vertex);
            let Ok(a) = move_vertex(p, &pid, &vid, dx, dy) else {
                return false;
            };
            let Ok(b) = move_vertex(&a, &pid, &vid, -dx, -dy) else {
                return false;
            };
            let laid_out = alter_py(ordered, &a, "move", Some(&pid), Some(&vid));
            let reread: Value = serde_json::from_str(&laid_out.dumps(Some(1))).unwrap();
            round_trip(p, &b)
                && history_len(&a) == history_len(p) + 1
                && history_len(&b) == history_len(p) + 2
                && a["meta"]["origin"] == "VECTOR_EDIT"
                && Pattern::new(a.clone()).structure_errors()
                    == Pattern::new(p.clone()).structure_errors()
                && same_data(&reread, &a)
        },
    );
}

#[test]
fn a_spread_and_its_inverse_give_the_pattern_back() {
    let pats = patterns();
    check(
        "alter: slash-spread then the opposite spread",
        0x5_1A54,
        300,
        |s: &mut Stream| {
            (
                s.next_u64(),
                (s.next_u64(), s.next_u64() % 2),
                (s.f64_in(0.05, 0.95), s.f64_in(0.5, 40.0), 0.0),
            )
        },
        |&(pat, (piece, axis), (frac, add, _))| {
            let (p, ordered) = &pats[(pat % pats.len() as u64) as usize];
            let (pid, _) = pick(p, piece, 0);
            let axis_name = if axis == 0 { "x" } else { "y" };
            let piece_doc = p["pieces"]
                .as_array()
                .unwrap()
                .iter()
                .find(|q| q["id"] == pid.as_str())
                .unwrap();
            let coords: Vec<f64> = piece_doc["vertices"]
                .as_object()
                .unwrap()
                .values()
                .filter_map(|v| v[axis as usize].as_f64())
                .collect();
            let (lo, hi) = coords
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), &c| {
                    (a.min(c), b.max(c))
                });
            // on the 0.001 mm grid, so the cut line never splits a rounded point from itself
            let at = ((lo + frac * (hi - lo)) * 1000.0).round() / 1000.0;
            let Ok(a) = slash_spread(p, &pid, axis_name, at, add) else {
                return false;
            };
            let Ok(b) = slash_spread(&a, &pid, axis_name, at, -add) else {
                return false;
            };
            let laid_out = alter_py(ordered, &a, "slash-spread", Some(&pid), None);
            let reread: Value = serde_json::from_str(&laid_out.dumps(Some(1))).unwrap();
            round_trip(p, &b)
                && history_len(&b) == history_len(p) + 2
                && Pattern::new(b.clone()).structure_errors()
                    == Pattern::new(p.clone()).structure_errors()
                && same_data(&reread, &a)
        },
    );
}
