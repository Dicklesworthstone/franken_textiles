//! Export text and number helpers against `tests/fixtures/oracle/export/helpers.json` (captured by
//! `tools/oracle/capture_exports.py`, spec 03 §2): `fnum`, `ascii_text`, `safe_name`, `text_width`,
//! `wrap`, `stroke_text`, column letters and page lists, all byte-exact.

use ftx_conformance::{Compare, fixture};
use ftx_export::{
    Anchor, ascii_text, column_letters, fnum, page_list, safe_name, stroke_text, text_width, wrap,
};
use serde_json::{Value, json};

#[test]
fn helpers_match_the_reference() {
    let fx = fixture("export", "helpers");
    let c = &fx["cases"];
    let mut cmp = Compare::new(0.0);
    let s = |v: &Value| v.as_str().unwrap().to_string();
    let f = |v: &Value| v.as_f64().unwrap();
    for (i, k) in c["fnum"].as_array().unwrap().iter().enumerate() {
        let got = fnum(f(&k["v"]), k["nd"].as_u64().unwrap() as usize);
        cmp.json(&format!("fnum[{i}]"), &json!(got), &k["out"]);
    }
    for k in c["text"].as_array().unwrap() {
        let input = s(&k["s"]);
        cmp.json(
            &format!("ascii_text({input:?})"),
            &json!(ascii_text(&input)),
            &k["ascii"],
        );
        for (key, n) in [("safe20", 20), ("safe5", 5), ("safe40", 40)] {
            cmp.json(
                &format!("safe_name({input:?}, {n})"),
                &json!(safe_name(&input, n)),
                &k[key],
            );
        }
    }
    for k in c["text_width"].as_array().unwrap() {
        let got = text_width(&s(&k["s"]), f(&k["size"]), k["bold"].as_bool().unwrap());
        cmp.num(&format!("text_width({})", k["s"]), got, f(&k["out"]));
    }
    for k in c["wrap"].as_array().unwrap() {
        let got = wrap(&s(&k["s"]), f(&k["size"]), f(&k["width"]));
        cmp.json(
            &format!("wrap({}, {})", k["size"], k["width"]),
            &json!(got),
            &k["out"],
        );
    }
    for k in c["stroke_text"].as_array().unwrap() {
        let anchor = match k["anchor"].as_str().unwrap() {
            "middle" => Anchor::Middle,
            "end" => Anchor::End,
            _ => Anchor::Start,
        };
        let got: Vec<Vec<[f64; 2]>> = stroke_text(
            &s(&k["s"]),
            f(&k["x"]),
            f(&k["y"]),
            f(&k["cap"]),
            anchor,
            k["y_down"].as_bool().unwrap(),
        )
        .into_iter()
        .map(|p| p.into_iter().map(|q| [q.x, q.y]).collect())
        .collect();
        cmp.json(&format!("stroke_text({})", k["s"]), &json!(got), &k["out"]);
    }
    for k in c["col"].as_array().unwrap() {
        let got = column_letters(k["c"].as_u64().unwrap() as usize);
        cmp.json(&format!("col({})", k["c"]), &json!(got), &k["out"]);
    }
    for k in c["page_list"].as_array().unwrap() {
        let nums: Vec<i64> = k["nums"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_i64().unwrap())
            .collect();
        cmp.json(
            &format!("page_list({nums:?})"),
            &json!(page_list(&nums)),
            &k["out"],
        );
    }
    cmp.finish("export helpers");
}
