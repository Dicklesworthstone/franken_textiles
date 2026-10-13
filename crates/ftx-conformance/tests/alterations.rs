//! Alteration ops against `tests/fixtures/oracle/alter/alterations.json` (captured by
//! `tools/oracle/capture_alterations.py`, spec 01 §10): slash-spread, move, true-seam, cap-ease and
//! transfer-curve on the drafted bolero and trousers of the reference wearers, a mutated bolero and the
//! error paths. The output document, its last history sentence and its structure errors must match, or
//! a refusal whose message contains the reference's (ftx names what is missing where the reference's
//! `KeyError` gives only the key).

use ftx_conformance::{Compare, fixture};
use ftx_core::Refusal;
use ftx_pattern::Pattern;
use ftx_pattern::alter::{cap_ease, move_vertex, slash_spread, transfer_curve, true_seam};
use serde_json::Value;

fn run(inp: &Value) -> Result<Value, Refusal> {
    let (pat, a) = (&inp["pattern"], &inp["args"]);
    let s = |k: &str| a[k].as_str().unwrap();
    let f = |k: &str| a[k].as_f64().unwrap();
    match inp["op"].as_str().unwrap() {
        "slash-spread" => slash_spread(pat, s("piece"), s("axis"), f("at"), f("add")),
        "move" => move_vertex(pat, s("piece"), s("vertex"), f("dx"), f("dy")),
        "true-seam" => true_seam(pat, s("seam_id"), s("end")),
        "cap-ease" => {
            let seams: Vec<String> = a["seams"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            cap_ease(pat, s("piece"), &seams, f("target"))
        }
        "transfer-curve" => transfer_curve(pat, &inp["source"], s("src_ref"), s("dst_ref")),
        op => panic!("unknown op {op}"),
    }
}

#[test]
fn alterations_match_the_reference() {
    let fx = fixture("alter", "alterations");
    let mut cmp = Compare::new(0.0);
    let mut failures = Vec::new();
    let cases = fx["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let want = &case["output"];
        match (run(&case["input"]), want.get("error")) {
            (Ok(doc), None) => {
                cmp.json(id, &doc, &want["doc"]);
                let why = doc["meta"]["history"]
                    .as_array()
                    .and_then(|h| h.last())
                    .map(|h| h["why"].clone());
                if why.as_ref() != Some(&want["why"]) {
                    failures.push(format!("{id}: why {why:?} vs {}", want["why"]));
                }
                let errs = Pattern::new(doc).structure_errors();
                if Value::from(errs.clone()) != want["structure_errors"] {
                    failures.push(format!(
                        "{id}: structure errors {errs:?} vs {}",
                        want["structure_errors"]
                    ));
                }
            }
            (Err(e), Some(err)) => {
                let want = err["message"].as_str().unwrap();
                if !e.message.contains(want) {
                    failures.push(format!(
                        "{id}: refused with {:?}, the reference raised {err}",
                        e.message
                    ));
                }
            }
            (Ok(_), Some(err)) => {
                failures.push(format!("{id}: no error, the reference raised {err}"))
            }
            (Err(e), None) => failures.push(format!("{id}: refused: {}", e.message)),
        }
    }
    println!("alterations: {} cases", cases.len());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    cmp.finish("alteration outputs");
}
