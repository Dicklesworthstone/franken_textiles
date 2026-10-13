//! The measurement evidence model against the reference's in `tests/fixtures/oracle/measure/` (captured by
//! `tools/oracle/capture_measurements.py`; spec 02 Part A without photos): chart priors across statures, ages
//! and sexes, the CDC table and its inverse, reference wearers, whole-document fusion of synthetic evidence,
//! the schema checks and readiness for every garment kind and option. Documents equal key for key with numbers
//! within 1e-9; refusals where the reference raises.

use ftx_conformance::fixture;
use ftx_core::Refusal;
use ftx_measure::{
    ReadinessOptions, age_from_stature, cdc_stature, fuse, priors_for, readiness, reference_wearer,
    validate,
};
use serde_json::{Value, json};

/// Structural equality, numbers within 1e-9; collects (path, got, want).
fn diff(a: &Value, b: &Value, path: &str, out: &mut Vec<String>) {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => {
            let (x, y) = (x.as_f64().unwrap(), y.as_f64().unwrap());
            if (x - y).abs() > 1e-9 * y.abs().max(1.0) {
                out.push(format!("{path}: {x} vs {y}"));
            }
        }
        (Value::Array(x), Value::Array(y)) if x.len() == y.len() => {
            for (i, (p, q)) in x.iter().zip(y).enumerate() {
                diff(p, q, &format!("{path}[{i}]"), out);
            }
        }
        (Value::Object(x), Value::Object(y)) => {
            for (k, q) in y {
                match x.get(k) {
                    Some(p) => diff(p, q, &format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k}: missing (want {})", short(q))),
                }
            }
            for k in x.keys().filter(|k| !y.contains_key(*k)) {
                out.push(format!("{path}.{k}: extra ({})", short(&x[k])));
            }
        }
        _ if a == b => {}
        _ => out.push(format!("{path}: {} vs {}", short(a), short(b))),
    }
}

fn short(v: &Value) -> String {
    let s = v.to_string();
    if s.len() > 140 {
        format!("{}…", &s[..140])
    } else {
        s
    }
}

fn opt_f(v: &Value) -> Option<f64> {
    v.as_f64()
}

/// Compares an ftx result with the reference's `{value}` or `{error, message}`.
fn check(id: &str, got: Result<Value, Refusal>, want: &Value, failures: &mut Vec<String>) -> usize {
    match (got, want.get("value")) {
        (Ok(g), Some(w)) => {
            let mut d = Vec::new();
            diff(&g, w, id, &mut d);
            let n = d.len();
            failures.extend(d.into_iter().take(6));
            n
        }
        (Err(_), None) => 0,
        (Err(r), Some(_)) => {
            failures.push(format!("{id}: refused ({r}), the reference answers"));
            1
        }
        (Ok(g), None) => {
            failures.push(format!(
                "{id}: answered {}, the reference raised {}: {}",
                short(&g),
                want["error"],
                want["message"]
            ));
            1
        }
    }
}

fn report(name: &str, cases: usize, differing: usize, failures: &[String]) {
    println!("{name}: {cases} cases, {differing} with differences");
    for f in failures.iter().take(40) {
        println!("  {f}");
    }
}

#[test]
fn priors_cdc_and_reference_wearers_match() {
    let fx = fixture("measure", "priors");
    let mut failures = Vec::new();
    let mut differing = 0;
    let cases = fx["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let i = &case["input"];
        let got = match case["kind"].as_str().unwrap() {
            "priors_for" => priors_for(
                i["stature_mm"].as_f64().unwrap(),
                opt_f(&i["age_months"]),
                i["sex"].as_str(),
            ),
            "cdc_stature" => cdc_stature(i["age_months"].as_f64().unwrap(), i["sex"].as_str())
                .map(|(a, b)| json!([a, b])),
            "age_from_stature" => {
                age_from_stature(i["stature_mm"].as_f64().unwrap()).map(|a| json!(a))
            }
            "reference_wearer" => reference_wearer(
                i["stature_mm"].as_f64().unwrap(),
                opt_f(&i["age_months"]),
                i["sex"].as_str(),
                None,
            ),
            k => panic!("{k}"),
        };
        if check(id, got, &case["output"], &mut failures) > 0 {
            differing += 1;
        }
    }
    report("priors", cases.len(), differing, &failures);
    assert!(failures.is_empty());
}

#[test]
fn fusion_and_schema_checks_match() {
    let fx = fixture("measure", "fusion");
    let mut failures = Vec::new();
    let mut differing = 0;
    let mut deviations = 0;
    let cases = fx["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let i = &case["input"];
        let want = &case["output"];
        let got = match case["kind"].as_str().unwrap() {
            "fuse" => fuse(&i["data"], i["use_priors"].as_bool().unwrap()).map(|mut g| {
                // the parity exception (see set_aside_wearer_stature_readiness): stature the reference left out
                let left_out = want["value"]["fused"].get("stature").is_none();
                if left_out
                    && i["data"]["wearer"]["stature_mm"].is_number()
                    && let Some(f) = g["fused"].as_object_mut()
                    && f.remove("stature").is_some()
                {
                    deviations += 1;
                }
                g
            }),
            "validate" => Ok(json!(validate(&i["data"]))),
            k => panic!("{k}"),
        };
        if check(id, got, want, &mut failures) > 0 {
            differing += 1;
        }
    }
    report("fusion", cases.len(), differing, &failures);
    println!(
        "  {deviations} fused documents where ftx keeps the wearer block's stature (parity exception)"
    );
    assert!(failures.is_empty());
}

#[test]
fn readiness_matches() {
    let fx = fixture("measure", "readiness");
    let mut failures = Vec::new();
    let mut differing = 0;
    let mut deviations = 0;
    let cases = fx["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let i = &case["input"];
        let o = &i["options"];
        let opts = ReadinessOptions {
            passes_hip: o["passes_hip"].as_bool().unwrap_or(false),
            closed_cuff: o["closed_cuff"].as_bool().unwrap_or(false),
            nappies: o["nappies"].as_bool(),
            elastic_waist: o["elastic_waist"].as_bool().unwrap_or(false),
            cuff_level: o["cuff_level"].as_str().unwrap_or("calf"),
            cuff_opening: o["cuff_opening"].as_bool().unwrap_or(false),
            use_priors: o["use_priors"].as_bool().unwrap_or(true),
        };
        let doc = &fx["docs"][i["doc"].as_str().unwrap()];
        let mut want = case["output"].clone();
        let got = readiness(
            doc,
            i["garment"].as_str().unwrap(),
            &opts,
            i["as_of"].as_str().unwrap(),
        )
        .map(|mut g| {
            if let Some(w) = want.get_mut("value")
                && set_aside_wearer_stature_readiness(doc, &mut g, w)
            {
                deviations += 1;
            }
            g
        });
        if check(id, got, &want, &mut failures) > 0 {
            differing += 1;
        }
    }
    report("readiness", cases.len(), differing, &failures);
    println!(
        "  {deviations} cases where ftx keeps the wearer block's stature as evidence (parity exception)"
    );
    assert!(failures.is_empty());
}

/// The one deliberate deviation (FEATURE_PARITY "Parity exceptions"): ftx keeps a numeric `wearer.stature_mm`
/// as stature evidence, while the reference applies its own wearer-block rule only when stature is already
/// being fused (an age given and priors on) and otherwise reports stature MISSING. Where the reference has it
/// MISSING and ftx does not, stature is set aside on both sides; a status that only stature decided too.
fn set_aside_wearer_stature_readiness(doc: &Value, got: &mut Value, want: &mut Value) -> bool {
    let missing_in_ref = want["items"].as_array().is_some_and(|it| {
        it.iter()
            .any(|i| i["name"] == "stature" && i["status"] == "MISSING")
    });
    let kept_by_ftx = got["items"].as_array().is_some_and(|it| {
        it.iter()
            .any(|i| i["name"] == "stature" && i["status"] != "MISSING")
    });
    if !(doc["wearer"]["stature_mm"].is_number() && missing_in_ref && kept_by_ftx) {
        return false;
    }
    let only_stature = want["blocking"] == json!(["stature"]);
    for v in [&mut *got, &mut *want] {
        if let Some(items) = v["items"].as_array_mut() {
            items.retain(|i| i["name"] != "stature");
        }
        if let Some(b) = v["blocking"].as_array_mut() {
            b.retain(|n| n != "stature");
        }
        if let Some(r) = v["remeasure_request"].as_array_mut() {
            r.retain(|s| !s.as_str().unwrap_or("").starts_with("stature:"));
        }
        if only_stature && let Some(o) = v.as_object_mut() {
            o.remove("status");
        }
    }
    true
}
