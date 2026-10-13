//! The post-trace commands against `tests/fixtures/oracle/trace/post.json` (captured by
//! `tools/oracle/capture_tracing.py` from drafts the reference traced off its own synthetic sheets; spec 04
//! §10.10–10.15): `parse_length`, `measure_ref` on traced, annotated and drafted patterns, `annotate`
//! (renames, cut, fold, mirroring, split, merge, seams with twins, checks, drops) and every refusal it
//! raises, `scale-fit`, and the round-0 audit with its label consensus.

use std::collections::BTreeMap;

use ftx_conformance::fixture_dir;
use ftx_pattern::Pattern;
use ftx_pattern::doc::PatternDoc;
use ftx_trace::{
    AuditOptions, OValue, annotate, audit, fit_scale, measure_ref, parse_length, parse_number,
};
use serde::Deserialize;
use serde_json::Value;

#[derive(Deserialize)]
struct Fixture {
    drafts: BTreeMap<String, PatternDoc>,
    annotated: BTreeMap<String, PatternDoc>,
    cases: Vec<Case>,
}

#[derive(Deserialize)]
struct Case {
    kind: String,
    id: String,
    input: OValue,
    output: Value,
}

fn load() -> Fixture {
    let path = fixture_dir().join("trace/post.json");
    let text = std::fs::read_to_string(&path).unwrap();
    serde_json::from_str(&text).unwrap()
}

impl Fixture {
    fn pattern(&self, r: &OValue) -> &PatternDoc {
        match (r.get("draft"), r.get("annotated")) {
            (Some(d), _) => &self.drafts[d.as_str().unwrap()],
            (_, Some(a)) => &self.annotated[a.as_str().unwrap()],
            _ => panic!("pattern ref {r:?}"),
        }
    }
}

thread_local! {
    /// (numbers compared, bit-identical, worst relative deviation)
    static STATS: std::cell::Cell<(usize, usize, f64)> = const { std::cell::Cell::new((0, 0, 0.0)) };
}

/// Structural equality, numbers within `tol` relative to max(1, |want|). Collects (path, got, want).
fn diff(a: &Value, b: &Value, path: &str, tol: f64, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            let rel = (x - y).abs() / y.abs().max(1.0);
            STATS.with(|s| {
                let (n, e, w) = s.get();
                s.set((n + 1, e + usize::from(x == y), w.max(rel)));
            });
            if (x - y).abs() > tol * y.abs().max(1.0) {
                out.push(format!("{path}: {x} vs {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(p, q, &format!("{path}[{i}]"), tol, out);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for k in x.keys().chain(y.keys().filter(|k| !x.contains_key(*k))) {
                match (x.get(k), y.get(k)) {
                    (Some(p), Some(q)) => diff(p, q, &format!("{path}.{k}"), tol, out),
                    (p, q) => out.push(format!("{path}.{k}: {p:?} vs {q:?}")),
                }
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {a} vs {b}")),
    }
}

fn doc_value(d: &PatternDoc) -> Value {
    serde_json::to_value(d).unwrap()
}

#[test]
fn post_trace_commands_match_the_reference() {
    let fx = load();
    let mut fails: Vec<String> = Vec::new();
    let mut seen: BTreeMap<&str, usize> = BTreeMap::new();
    for case in &fx.cases {
        *seen.entry(case.kind.as_str()).or_default() += 1;
        let input = &case.input;
        let mut out = Vec::new();
        match case.kind.as_str() {
            "parse_length" => {
                let unit = input.get("default_unit").and_then(OValue::as_str);
                let got = match input.get("number") {
                    Some(n) => parse_number(n.as_f64().unwrap(), unit),
                    None => parse_length(input.get("text").and_then(OValue::as_str).unwrap(), unit),
                };
                diff(&serde_json::json!(got), &case.output, "", 0.0, &mut out);
            }
            "measure_ref" => {
                let p = Pattern::from_doc(fx.pattern(input.get("pattern").unwrap()).clone());
                let got = match measure_ref(&p, input.get("ref").and_then(OValue::as_str).unwrap())
                {
                    Ok(l) => serde_json::json!({"ok": l}),
                    Err(e) => serde_json::json!({"message": e.to_string()}),
                };
                let mut want = case.output.clone();
                if let Some(m) = want.as_object_mut() {
                    m.remove("error");
                }
                diff(&got, &want, "", 0.0, &mut out);
            }
            "annotate" => {
                let draft = fx.pattern(input.get("pattern").unwrap());
                match (
                    annotate(draft, input.get("names").unwrap()),
                    case.output.get("annotated"),
                ) {
                    (Ok(doc), Some(id)) => {
                        diff(
                            &doc_value(&doc),
                            &doc_value(&fx.annotated[id.as_str().unwrap()]),
                            "",
                            0.0,
                            &mut out,
                        );
                    }
                    (Err(e), None) => diff(
                        &Value::String(e),
                        &case.output["message"],
                        "",
                        0.0,
                        &mut out,
                    ),
                    (Ok(_), None) => out.push(format!(
                        "accepted; the reference refused: {}",
                        case.output["message"]
                    )),
                    (Err(e), Some(_)) => out.push(format!("refused: {e}")),
                }
            }
            "scale_fit" => {
                let doc = fx.pattern(input.get("pattern").unwrap());
                let targets: Vec<(String, f64)> = input
                    .get("targets")
                    .unwrap()
                    .items()
                    .iter()
                    .map(|t| {
                        (
                            t.items()[0].as_str().unwrap().to_string(),
                            t.items()[1].as_f64().unwrap(),
                        )
                    })
                    .collect();
                match (fit_scale(doc, &targets), case.output.get("ok")) {
                    (Ok((scaled, report)), Some(ok)) => {
                        let id = ok["pattern"]["annotated"].as_str().unwrap();
                        diff(
                            &doc_value(&scaled),
                            &doc_value(&fx.annotated[id]),
                            "",
                            0.0,
                            &mut out,
                        );
                        diff(&report, &ok["report"], ".report", 0.0, &mut out);
                    }
                    (Err(e), None) => diff(
                        &Value::String(e.to_string()),
                        &case.output["message"],
                        "",
                        0.0,
                        &mut out,
                    ),
                    (Ok(_), None) => out.push(format!(
                        "accepted; the reference refused: {}",
                        case.output["message"]
                    )),
                    (Err(e), Some(_)) => out.push(format!("refused: {e}")),
                }
            }
            "audit" => {
                let doc = fx.pattern(input.get("pattern").unwrap());
                let labels: Option<Vec<Value>> = input
                    .get("labels")
                    .filter(|l| !matches!(l, OValue::Null))
                    .map(|l| l.items().iter().map(OValue::to_value).collect());
                let opt = input.get("options").unwrap();
                let mut o = AuditOptions::default();
                for (k, slot) in [
                    ("tol_mm", &mut o.tol_mm),
                    ("median_seam_mm", &mut o.median_seam_mm),
                    ("label_tol", &mut o.label_tol),
                    ("fold_tol_deg", &mut o.fold_tol_deg),
                ] {
                    if let Some(v) = opt.get(k).and_then(OValue::as_f64) {
                        *slot = v;
                    }
                }
                let report = audit(doc, labels.as_deref(), &o);
                diff(&report, &case.output["ok"], "", 0.0, &mut out);
            }
            other => panic!("unknown case kind {other}"),
        }
        fails.extend(
            out.into_iter()
                .take(6)
                .map(|m| format!("{} {}: {m}", case.kind, case.id)),
        );
    }
    let (n, exact, worst) = STATS.with(std::cell::Cell::get);
    println!(
        "cases: {seen:?}; {exact} of {n} numbers bit-identical, worst relative deviation {worst:.1e}"
    );
    assert!(
        fails.is_empty(),
        "{} mismatches:\n{}",
        fails.len(),
        fails.join("\n")
    );
}
