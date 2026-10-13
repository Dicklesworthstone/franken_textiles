//! Validation against `tests/fixtures/oracle/validate/` (captured by `tools/oracle/capture_validation.py`):
//! the report JSON and the exact text table for the nine drafted garments, the mutation tests M1–M12, the
//! seam-walk, truing and point-of-measure fixtures (spec 01 F12–F32), and the role-based default fit spec.

use ftx_conformance::{Compare, fixture};
use ftx_pattern::validate::BodyMeasure;
use ftx_pattern::{Measurements, Pattern, PyError, default_fit_spec, format_table, validate};
use serde_json::Value;

/// Measurement values from the file, landmark heights from the stub recorded in the case (F30).
struct Stub {
    meas: Measurements,
    landmarks: Value,
}

impl BodyMeasure for Stub {
    fn value(&self, name: &str) -> Result<f64, PyError> {
        self.meas.value(name)
    }
    fn sigma(&self, name: &str) -> Option<f64> {
        self.meas.sigma(name)
    }
    fn landmark_h(&self, part_key: &str, name: &str) -> Result<f64, PyError> {
        self.landmarks[part_key][name]
            .as_f64()
            .ok_or_else(|| PyError::Key(name.to_string()))
    }
}

#[test]
fn reports_and_tables_match_the_reference() {
    let fx = fixture("validate", "validation");
    let mut cmp = Compare::new(1e-12);
    let mut table_failures = Vec::new();
    let cases = fx["cases"].as_array().unwrap();
    for case in cases {
        let id = case["id"].as_str().unwrap();
        let inp = &case["input"];
        let pat = Pattern::new(inp["pattern"].clone());
        let meas = Measurements(inp["measurements"].clone());
        let rep = match inp.get("landmarks_stub") {
            Some(stub) => validate(
                &pat,
                &inp["fit_spec"],
                &Stub {
                    meas,
                    landmarks: stub.clone(),
                },
            ),
            None => validate(&pat, &inp["fit_spec"], &meas),
        };
        cmp.json(
            id,
            &serde_json::to_value(&rep).unwrap(),
            &case["output"]["report"],
        );
        let table = format_table(&rep);
        let want = case["output"]["table"].as_str().unwrap();
        if table != want {
            let diff: Vec<String> = table
                .lines()
                .zip(want.lines())
                .filter(|(a, b)| a != b)
                .take(4)
                .map(|(a, b)| format!("    got  {a:?}\n    want {b:?}"))
                .collect();
            table_failures.push(format!("{id}:\n{}", diff.join("\n")));
        }
    }
    println!(
        "validation: {} cases, {} tables",
        cases.len(),
        cases.len() - table_failures.len()
    );
    assert!(
        table_failures.is_empty(),
        "table mismatches:\n{}",
        table_failures.join("\n")
    );
    cmp.finish("validation reports");
}

#[test]
fn default_fit_specs_match_the_reference() {
    let fx = fixture("validate", "default_fit_spec");
    let mut cmp = Compare::new(0.0);
    for case in fx["cases"].as_array().unwrap() {
        let id = case["id"].as_str().unwrap();
        let garment = id.rsplit_once('-').map_or(id, |(g, _)| g);
        let pat = Pattern::new(case["input"]["pattern"].clone());
        cmp.json(id, &default_fit_spec(&pat, garment), &case["output"]);
    }
    cmp.finish("default fit specs");
}
