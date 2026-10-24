//! `tests/fixtures/oracle/web/webapp.json` (captured by `tools/oracle/capture_webapp.py` from the reference's
//! `webapp.py` with the clock at 2026-10-13 12:00) against `ftx_wasm::page`: program_info, form_fields and
//! run, what the page shows and the documents written, every refusal's message.

use std::path::Path;

use ftx_export::PyJson;
use serde_json::{Value, json};

const TODAY: &str = "2026-10-13";
const CREATED: &str = "2026-10-13T12:00";

fn get<'a>(v: &'a PyJson, k: &str) -> &'a PyJson {
    v.get(k).unwrap_or_else(|| panic!("missing {k}"))
}

/// A JSON tree with every float rounded to 1e-9 relative (the oracle's numbers went through Python's repr).
fn close(a: &Value, b: &Value, path: &str, fails: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > 1e-9 * x.abs().max(y.abs()).max(1.0) {
                fails.push(format!("{path}: {x} vs {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                close(p, q, &format!("{path}[{i}]"), fails);
            }
        }
        (Value::Object(x), Value::Object(y)) if x.len() == y.len() => {
            for (k, p) in x {
                match y.get(k) {
                    Some(q) => close(p, q, &format!("{path}.{k}"), fails),
                    None => fails.push(format!("{path}: key {k} only in ftx")),
                }
            }
        }
        _ if a == b => {}
        _ => fails.push(format!("{path}: {} vs {}", short(a), short(b))),
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 160 {
        format!("{}…", &s[..160])
    } else {
        s
    }
}

#[test]
fn the_page_round_matches_webapp_py() {
    let path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/oracle/web/webapp.json");
    let doc = PyJson::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    let PyJson::List(cases) = get(&doc, "cases") else {
        panic!("cases")
    };
    let mut fails = Vec::new();
    for case in cases {
        let PyJson::Str(id) = get(case, "id") else {
            panic!("id")
        };
        let PyJson::Str(f) = get(case, "fn") else {
            panic!("fn")
        };
        let (input, want) = (get(case, "input"), get(case, "output").to_value());
        let mut here = Vec::new();
        match f.as_str() {
            "program_info" => close(
                &ftx_wasm::page::program_info().to_value(),
                &want,
                id,
                &mut here,
            ),
            "form_fields" => {
                let i = input.to_value();
                let got = ftx_wasm::page::form_fields(
                    i["program"].as_str(),
                    i["stature_cm"].as_f64().unwrap(),
                    i["age_months"].as_f64(),
                    i["nappies"].as_bool().unwrap(),
                    TODAY,
                );
                match got {
                    Ok(rows) => close(&Value::Array(rows), &want, id, &mut here),
                    Err(e) => here.push(format!("{id}: refused {e}")),
                }
            }
            "run" => match (
                ftx_wasm::page::run(&input.dumps(None), TODAY, CREATED),
                want.get("error"),
            ) {
                (Err(e), Some(_)) => {
                    if e != want["message"] {
                        here.push(format!("{id}: message {e:?} vs {}", want["message"]));
                    }
                }
                (Err(e), None) => here.push(format!("{id}: refused {e}")),
                (Ok(_), Some(_)) => here.push(format!(
                    "{id}: ran; the reference refused: {}",
                    want["message"]
                )),
                (Ok(round), None) => {
                    let mut page = round.page.to_value();
                    if let Some(o) = page.as_object_mut() {
                        o.remove("zip");
                        if let Some(svg) = o.get("preview_svg").map(|s| !s.is_null()) {
                            o.insert("preview_svg".into(), json!(svg));
                        }
                        if let Some(Value::Array(files)) = o.get_mut("files") {
                            for f in files {
                                f.as_object_mut().unwrap().remove("bytes");
                            }
                        }
                    }
                    close(&page, &want["page"], &format!("{id} page"), &mut here);
                    for (name, w) in want["documents"].as_object().into_iter().flatten() {
                        let got = round.files.iter().find(|(n, _)| n == name).map(|(_, b)| b);
                        match got {
                            Some(b) => {
                                let g: Value = serde_json::from_slice(b).unwrap();
                                close(&g, w, &format!("{id} {name}"), &mut here);
                            }
                            None => here.push(format!("{id}: no {name}")),
                        }
                    }
                }
            },
            other => panic!("{other}"),
        }
        if here.is_empty() {
            println!("{id}: same");
        } else {
            println!("{id}: {} differences, first {}", here.len(), here[0]);
        }
        fails.extend(here);
    }
    assert!(
        fails.is_empty(),
        "{} differences:\n{}",
        fails.len(),
        fails
            .iter()
            .take(40)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
}

fn file_request() -> Value {
    let oracle: Value = serde_json::from_str(include_str!(
        "../../../tests/fixtures/oracle/web/webapp.json"
    ))
    .unwrap();
    oracle["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|case| case["id"] == "file-pattern-spec")
        .unwrap()["input"]
        .clone()
}

#[test]
fn uploaded_records_override_a_poisoned_cache_in_every_round_document() {
    let fresh = file_request();
    let expected = ftx_wasm::page::run(&fresh.to_string(), TODAY, CREATED).unwrap();
    let mut stale = fresh.clone();
    stale["body"]["measurements"]["fused"]["chest"]["value_mm"] = json!(1111.0);
    stale["body"]["measurements"]["fused"]["waist"]["value_mm"] = json!(1234.0);
    let original = stale.clone();
    let got = ftx_wasm::page::run(&stale.to_string(), TODAY, CREATED).unwrap();
    assert!(
        got.files == expected.files,
        "round file bytes changed after cache poisoning"
    );
    assert!(
        got.zip == expected.zip,
        "ZIP bytes changed after cache poisoning"
    );
    assert!(
        got.page == expected.page,
        "page changed after cache poisoning"
    );
    assert_eq!(stale, original, "an upload is not rewritten");
}

#[test]
fn an_edited_record_drives_the_uploaded_body_and_bad_records_still_refuse() {
    let mut request = file_request();
    let data = &mut request["body"]["measurements"];
    data["wearer"]["reference"] = json!(false);
    let chest = data["records"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|r| r["name"] == "chest")
        .unwrap();
    chest["value_mm"] = json!(620.0);
    chest["sigma_mm"] = json!(1.0);
    chest["provenance"] = json!("MEASURED_TAPE");
    chest["method"] = json!("test: synthetic edited tape target");
    let original = request.clone();
    let body = PyJson::from_value(&request["body"]);
    let got = ftx_wasm::page::body_file(&body, TODAY).unwrap();
    assert!(
        got["fused"]["chest"]["value_mm"].as_f64().unwrap() > 600.0,
        "{got}"
    );
    assert_eq!(got["fused"]["chest"]["primary_provenance"], "MEASURED_TAPE");
    assert_eq!(request, original);
    request["body"]["measurements"]["records"][0]["sigma_mm"] = json!(-1.0);
    let refused = ftx_wasm::page::run(&request.to_string(), TODAY, CREATED).unwrap_err();
    assert!(refused.contains("sigma"), "{refused}");
}

#[test]
fn a_ready_cache_cannot_promote_model_records_to_independent_evidence() {
    let mut request = file_request();
    let data = &mut request["body"]["measurements"];
    data["wearer"]["reference"] = json!(false);
    for record in data["records"].as_array_mut().unwrap() {
        record["provenance"] = json!("MEASURED_TAPE");
        record["method"] = json!("test: synthetic independent tape control");
        let sigma = if record.get("sigma_deg").is_some() {
            "sigma_deg"
        } else {
            "sigma_mm"
        };
        record[sigma] = json!(1.0);
    }
    let measured = ftx_measure::fuse(data, true).unwrap();
    *data = measured;
    let control = ftx_wasm::page::run(&request.to_string(), TODAY, CREATED).unwrap();
    assert_eq!(control.page.to_value()["readiness"]["status"], "READY");
    for record in request["body"]["measurements"]["records"]
        .as_array_mut()
        .unwrap()
    {
        record["provenance"] = json!("MODEL_FIT");
        record["method"] = json!("test: model estimate, not an independent measurement");
    }
    let got = ftx_wasm::page::run(&request.to_string(), TODAY, CREATED).unwrap();
    assert_eq!(got.page.to_value()["readiness"]["status"], "NOT_READY");
    assert_eq!(got.page.to_value()["status"], "MEASUREMENTS_NOT_READY");
    let bytes = &got
        .files
        .iter()
        .find(|(name, _)| name == "measurements.json")
        .unwrap()
        .1;
    let saved: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(saved["fused"]["chest"]["primary_provenance"], "MODEL_FIT");
}
