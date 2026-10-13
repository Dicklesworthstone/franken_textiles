//! Oracle fixtures (`tests/fixtures/oracle/<subsystem>/<name>.json`, written by `tools/oracle/`) and the
//! comparison the conformance tests share (plan §17.1).
//!
//! A fixture is `{"schema": "ftx-oracle/1", "subsystem", "fixture", "meta", "cases": [{"id", "fn",
//! "input", "output"}]}`. Fixtures are captured from the reference only; never regenerate them from
//! this code.
#![forbid(unsafe_code)]

use std::path::PathBuf;

use serde_json::Value;

/// The workspace's oracle fixture directory.
#[must_use]
pub fn fixture_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/oracle")
}

/// Loads `tests/fixtures/oracle/<subsystem>/<name>.json`.
///
/// # Panics
/// If the file is missing or not JSON (a broken checkout, not a test result).
#[must_use]
pub fn fixture(subsystem: &str, name: &str) -> Value {
    let path = fixture_dir().join(subsystem).join(format!("{name}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let v: Value =
        serde_json::from_str(&text).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    assert_eq!(v["schema"], "ftx-oracle/1", "{}", path.display());
    v
}

/// Numeric comparison with a relative tolerance and a record of the worst deviation and of how many
/// values matched bit for bit.
#[derive(Debug)]
pub struct Compare {
    /// Relative tolerance: |got − want| ≤ tol · max(1, |want|).
    pub tol: f64,
    /// Largest |got − want| / max(1, |want|) seen.
    pub worst: f64,
    /// Numbers compared.
    pub count: usize,
    /// Numbers equal bit for bit (−0.0 and 0.0 count as equal).
    pub exact: usize,
    /// Mismatch descriptions.
    pub failures: Vec<String>,
}

impl Compare {
    /// A comparison with this relative tolerance.
    #[must_use]
    pub fn new(tol: f64) -> Self {
        Self {
            tol,
            worst: 0.0,
            count: 0,
            exact: 0,
            failures: Vec::new(),
        }
    }

    /// Compares two numbers.
    pub fn num(&mut self, what: &str, got: f64, want: f64) {
        self.count += 1;
        if got == want {
            self.exact += 1;
            return;
        }
        let d = (got - want).abs() / want.abs().max(1.0);
        if d.is_nan() || d > self.tol {
            self.failures.push(format!(
                "{what}: got {got:e}, want {want:e} (relative {d:.2e})"
            ));
        }
        if d.is_finite() {
            self.worst = self.worst.max(d);
        }
    }

    /// Compares a number list against a JSON array.
    pub fn nums(&mut self, what: &str, got: &[f64], want: &Value) {
        let w = want.as_array().map(Vec::as_slice).unwrap_or_default();
        if got.len() != w.len() {
            self.failures
                .push(format!("{what}: {} values, want {}", got.len(), w.len()));
            return;
        }
        for (i, (g, v)) in got.iter().zip(w).enumerate() {
            self.num(&format!("{what}[{i}]"), *g, v.as_f64().unwrap_or(f64::NAN));
        }
    }

    /// Compares two JSON values structurally, numbers within tolerance.
    pub fn json(&mut self, what: &str, got: &Value, want: &Value) {
        match (got, want) {
            (Value::Number(g), Value::Number(w)) => {
                self.num(
                    what,
                    g.as_f64().unwrap_or(f64::NAN),
                    w.as_f64().unwrap_or(f64::NAN),
                );
            }
            (Value::Array(g), Value::Array(w)) if g.len() == w.len() => {
                for (i, (a, b)) in g.iter().zip(w).enumerate() {
                    self.json(&format!("{what}[{i}]"), a, b);
                }
            }
            (Value::Object(g), Value::Object(w)) if g.len() == w.len() => {
                for (k, b) in w {
                    match g.get(k) {
                        Some(a) => self.json(&format!("{what}.{k}"), a, b),
                        None => self.failures.push(format!("{what}: missing key {k}")),
                    }
                }
            }
            (g, w) if g == w => {}
            (g, w) => self.failures.push(format!("{what}: got {g}, want {w}")),
        }
    }

    /// Panics with every mismatch, after printing the summary line.
    ///
    /// # Panics
    /// If any comparison failed.
    pub fn finish(&self, name: &str) {
        println!(
            "{name}: {} numbers, {} bit-exact ({:.1} %), worst relative deviation {:.2e} (tolerance {:.0e})",
            self.count,
            self.exact,
            100.0 * self.exact as f64 / self.count.max(1) as f64,
            self.worst,
            self.tol
        );
        let shown: Vec<&str> = self.failures.iter().take(40).map(String::as_str).collect();
        assert!(
            self.failures.is_empty(),
            "{name}: {} mismatches:\n{}",
            self.failures.len(),
            shown.join("\n")
        );
    }
}
