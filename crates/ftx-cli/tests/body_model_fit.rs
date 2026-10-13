//! `ftx body` reports what it was not given as `MODEL_FIT` (plan §9.4): every tape reading the measurement
//! file lacks comes back as a `fabric-measurements/1` record read off the fitted body, with the population
//! spread as σ, and fusing it keeps it a model value, never a measurement.
//!
//! Needs the converted Anny model, so ignored by default:
//! `cargo test -p ftx-cli --release --test body_model_fit -- --include-ignored`.

use std::path::Path;
use std::process::Command;

use serde_json::{Value, json};

#[test]
#[ignore = "needs the converted Anny model"]
fn unmeasured_readings_come_back_as_model_fit() {
    let dir = std::env::temp_dir().join(format!("ftx-body-model-fit-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/oracle/draft/programs.json");
    let fx: Value = serde_json::from_slice(&std::fs::read(fixture).unwrap()).unwrap();
    let mut meas = fx["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "zouave_bolero-920")
        .unwrap()["input"]["measurements"]
        .clone();
    // a wearer measured only round the trunk
    let kept: serde_json::Map<String, Value> = meas["fused"]
        .as_object()
        .unwrap()
        .iter()
        .filter(|(k, _)| ["stature", "chest", "waist", "hip"].contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    meas["fused"] = Value::Object(kept);
    meas["wearer"]["reference"] = json!(false);
    std::fs::write(dir.join("m.json"), serde_json::to_vec(&meas).unwrap()).unwrap();
    let out = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(&dir)
        .args([
            "body",
            "--measurements",
            "m.json",
            "--out",
            "body",
            "--json",
        ])
        .output()
        .unwrap();
    assert_eq!(
        out.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let r: Value = serde_json::from_slice(&out.stdout).unwrap();
    let body = &r["result"]["body"];
    let fitted: Vec<&str> = body["fit"]["tape"]
        .as_object()
        .unwrap()
        .keys()
        .map(String::as_str)
        .collect();
    let records = body["model_fit"].as_array().unwrap();
    let names: Vec<&str> = records
        .iter()
        .map(|m| m["name"].as_str().unwrap())
        .collect();
    for n in ftx_body::TAPE_FIELDS {
        assert!(
            fitted.contains(&n) != names.contains(&n),
            "{n}: fitted {fitted:?}, model_fit {names:?}"
        );
    }
    for m in records {
        assert_eq!(m["provenance"], "MODEL_FIT");
        assert!(
            m["sigma_mm"].as_f64().unwrap() > 0.0 && m["value_mm"].as_f64().unwrap() > 0.0,
            "{m}"
        );
        assert_eq!(
            body["readings_mm"][m["name"].as_str().unwrap()],
            m["value_mm"]
        );
    }
    // fused, a model value stays a model value: never a direct (tape) source
    let mut doc = meas.clone();
    doc["records"] = Value::Array(records.clone());
    let fused = ftx_measure::fuse(&doc, false).unwrap();
    assert_eq!(fused["fused"]["head"]["primary_provenance"], "MODEL_FIT");
}
