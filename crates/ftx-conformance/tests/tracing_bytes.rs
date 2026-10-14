//! `annotate` and `scale-fit` as the reference writes them. The oracle `trace/post.json` keeps the reference's
//! documents in their key order, so each result must be the reference's `json.dump(…, indent=1)` byte for
//! byte: key order, integers kept as integers, floats as `repr`.

use ftx_conformance::fixture_dir;
use ftx_export::pyjson::PyJson;
use ftx_pattern::doc::PatternDoc;
use ftx_trace::{OValue, annotate_py, fit_scale_py};

fn first_difference(got: &str, want: &str) -> String {
    let (g, w): (Vec<&str>, Vec<&str>) = (got.lines().collect(), want.lines().collect());
    let at = (0..g.len().max(w.len()))
        .find(|&i| g.get(i) != w.get(i))
        .unwrap_or(0);
    format!(
        "line {}: got {:?}, want {:?}",
        at + 1,
        g.get(at).unwrap_or(&""),
        w.get(at).unwrap_or(&"")
    )
}

fn text(v: Option<&PyJson>) -> &str {
    match v {
        Some(PyJson::Str(s)) => s,
        _ => panic!("expected a string, got {v:?}"),
    }
}

#[test]
fn annotate_and_scale_fit_write_the_reference_bytes() {
    let raw = std::fs::read_to_string(fixture_dir().join("trace/post.json")).unwrap();
    let fx = PyJson::parse(&raw).unwrap();
    let (drafts, annotated) = (fx.get("drafts").unwrap(), fx.get("annotated").unwrap());
    let pattern = |r: &PyJson| -> &PyJson {
        match r.get("draft") {
            Some(d) => drafts.get(text(Some(d))).unwrap(),
            None => annotated.get(text(r.get("annotated"))).unwrap(),
        }
    };
    // parsed from the text, as the CLI reads it, so the vertex order is the document's
    let typed = |p: &PyJson| -> PatternDoc { serde_json::from_str(&p.dumps(Some(1))).unwrap() };
    let Some(PyJson::List(cases)) = fx.get("cases") else {
        panic!("no cases")
    };
    let (mut checked, mut fails) = (0, Vec::new());
    for case in cases {
        let id = text(case.get("id"));
        let (input, output) = (case.get("input").unwrap(), case.get("output").unwrap());
        match text(case.get("kind")) {
            "annotate" => {
                let Some(want) = output.get("annotated") else {
                    continue;
                };
                let draft = pattern(input.get("pattern").unwrap());
                let names: OValue =
                    serde_json::from_str(&input.get("names").unwrap().dumps(None)).unwrap();
                let got = annotate_py(draft, &typed(draft), &names).unwrap().0;
                let want = annotated.get(text(Some(want))).unwrap().dumps(Some(1));
                let got = got.dumps(Some(1));
                if got != want {
                    fails.push(format!("{id}: {}", first_difference(&got, &want)));
                }
                checked += 1;
            }
            "scale_fit" => {
                let Some(ok) = output.get("ok") else {
                    continue;
                };
                let pat = pattern(input.get("pattern").unwrap());
                let Some(PyJson::List(given)) = input.get("targets") else {
                    panic!("{id}: targets")
                };
                let targets: Vec<(String, f64)> = given
                    .iter()
                    .map(|t| match t {
                        PyJson::List(pair) => (
                            text(pair.first()).to_string(),
                            match pair.get(1) {
                                Some(PyJson::Float(x)) => *x,
                                Some(PyJson::Int(i)) => *i as f64,
                                other => panic!("{id}: target length {other:?}"),
                            },
                        ),
                        other => panic!("{id}: target {other:?}"),
                    })
                    .collect();
                let (doc, report, _) = fit_scale_py(pat, &typed(pat), &targets).unwrap();
                let want_doc = pattern(ok.get("pattern").unwrap()).dumps(Some(1));
                let want_report = ok.get("report").unwrap().dumps(Some(1));
                for (what, got, want) in [
                    ("pattern", doc.dumps(Some(1)), want_doc),
                    ("report", report.dumps(Some(1)), want_report),
                ] {
                    if got != want {
                        fails.push(format!("{id} {what}: {}", first_difference(&got, &want)));
                    }
                }
                checked += 1;
            }
            _ => {}
        }
    }
    println!("{checked} annotate and scale-fit results compared byte for byte");
    assert!(checked >= 17, "only {checked} cases");
    assert!(
        fails.is_empty(),
        "{} differ from the reference:\n{}",
        fails.len(),
        fails.join("\n")
    );
}
