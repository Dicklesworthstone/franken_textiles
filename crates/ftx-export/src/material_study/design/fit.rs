//! Explicit flat-pattern fit bands against a caller-supplied, selected-body measurement authority.
//! The canonical validator decides every measurement and seam; this gate adds no fit score or
//! inferred ease allowance, and its result is not evidence of fit on a real wearer.

use std::collections::BTreeSet;

use ftx_core::Refusal;
use ftx_pattern::{
    Pattern,
    validate::{self, BodyMeasure},
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::{Outcome, budget, invalid};

const MAX_POINTS: usize = 32;
const MAX_SELECTORS: usize = 64;
const MAX_SPEC_BYTES: usize = 256 * 1024;
const MAX_IDENTITY_BYTES: usize = 256 * 1024;

/// Declared points of measure and ease bands; no default fit specification is synthesized.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    /// Existing `fabric-fit/1` specification, retaining its labels, units and explicit targets.
    pub fit_spec: Value,
}

/// Trusted body context supplied by a production adapter, independently of request JSON.
pub struct Context<'a> {
    /// Measurements and part-frame landmarks of the selected fitted body.
    pub body: &'a dyn BodyMeasure,
    /// Exact model/shape/body-source identity supplied by that adapter, retained unchanged.
    pub identity: &'a Value,
}

impl Context<'_> {
    fn validate(&self) -> Result<(), Refusal> {
        if self
            .identity
            .as_object()
            .is_none_or(|identity| identity.is_empty())
        {
            return Err(invalid(
                "design fit body identity must be a nonempty object",
            ));
        }
        let bytes = serde_json::to_vec(self.identity).map_err(|error| {
            invalid(format!(
                "cannot serialize design fit body identity: {error}"
            ))
        })?;
        if bytes.len() > MAX_IDENTITY_BYTES {
            return Err(budget("design fit body identity exceeds 256 KiB"));
        }
        Ok(())
    }
}

/// Canonical flat-pattern measurements, including out-of-band and unavailable readings.
#[derive(Clone, Debug, Serialize)]
pub struct Report {
    /// Exact identity of the body authority used to evaluate this candidate.
    pub body_identity: Value,
    /// Exact validated input specification; negative ease is allowed only when explicitly stated.
    pub fit_spec: Value,
    /// Unmodified canonical structural, seam, truing, completeness and point-of-measure report.
    pub validation: Value,
    /// True only when the canonical report is `CONVERGED`, including all declared fit bands.
    pub passed: bool,
}

fn nonempty<'a>(value: &'a Value, key: &str) -> Result<&'a str, Refusal> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or_else(|| invalid(format!("design fit needs a nonempty string {key}")))
}

fn selectors(point: &Value, field: &str) -> Result<(), Refusal> {
    let values = point
        .get(field)
        .and_then(Value::as_array)
        .filter(|values| !values.is_empty())
        .ok_or_else(|| invalid(format!("design fit point needs a nonempty {field} array")))?;
    if values.len() > MAX_SELECTORS {
        return Err(budget(
            "design fit point exceeds 64 instance/path selectors",
        ));
    }
    if values
        .iter()
        .any(|value| value.as_str().is_none_or(|s| s.trim().is_empty()))
    {
        return Err(invalid(format!(
            "design fit {field} must contain nonempty strings"
        )));
    }
    Ok(())
}

fn optional_number(value: &Value, field: &str, nonnegative: bool) -> Result<(), Refusal> {
    if let Some(raw) = value.get(field) {
        let number = raw
            .as_f64()
            .filter(|number| number.is_finite())
            .ok_or_else(|| invalid(format!("design fit {field} must be finite")))?;
        if nonnegative && number < 0.0 {
            return Err(invalid(format!("design fit {field} must be nonnegative")));
        }
    }
    Ok(())
}

fn validate_point(point: &Value, ids: &mut BTreeSet<String>) -> Result<(), Refusal> {
    let id = nonempty(point, "id")?;
    if !ids.insert(id.to_owned()) {
        return Err(invalid("design fit point IDs must be unique"));
    }
    nonempty(point, "body")?;
    let target = point
        .get("target_mm")
        .and_then(Value::as_array)
        .filter(|target| target.len() == 3)
        .ok_or_else(|| {
            invalid("design fit target_mm must have exactly three finite ordered values")
        })?;
    let mut previous = f64::NEG_INFINITY;
    for value in target {
        let number = value
            .as_f64()
            .filter(|number| number.is_finite())
            .ok_or_else(|| {
                invalid("design fit target_mm must have exactly three finite ordered values")
            })?;
        if number < previous {
            return Err(invalid(
                "design fit target_mm must satisfy minimum <= nominal <= maximum",
            ));
        }
        previous = number;
    }
    for field in ["body_scale", "body_offset_mm", "dy", "scale"] {
        optional_number(point, field, false)?;
    }
    match nonempty(point, "kind")? {
        "girth" => {
            selectors(point, "instances")?;
            nonempty(point, "landmark")?;
        }
        "width" => {
            nonempty(point, "instance")?;
            if point.get("at_vertex").is_some() {
                nonempty(point, "at_vertex")?;
            } else {
                nonempty(point, "landmark")?;
            }
        }
        "vertical" => {
            for field in ["instance", "from", "to"] {
                nonempty(point, field)?;
            }
        }
        "path" => selectors(point, "refs")?,
        _ => {
            return Err(invalid(
                "design fit supports girth, width, vertical and path points",
            ));
        }
    }
    Ok(())
}

impl Request {
    /// Validate the bounded specification before any alterations or body-dependent evaluations.
    /// Pattern/body names are resolved by the canonical validator, retaining unavailable readings.
    ///
    /// # Errors
    /// Refuses malformed, unordered, non-finite or oversized point specifications.
    pub fn validate(&self) -> Result<(), Refusal> {
        if !self.fit_spec.is_object() {
            return Err(invalid("design fit_spec must be an object"));
        }
        if self
            .fit_spec
            .get("schema")
            .is_some_and(|schema| schema != "fabric-fit/1")
        {
            return Err(invalid(
                "design fit_spec schema must be fabric-fit/1 when supplied",
            ));
        }
        let bytes = serde_json::to_vec(&self.fit_spec)
            .map_err(|error| invalid(format!("cannot serialize design fit_spec: {error}")))?;
        if bytes.len() > MAX_SPEC_BYTES {
            return Err(budget("design fit_spec exceeds 256 KiB"));
        }
        let points = self
            .fit_spec
            .get("points")
            .and_then(Value::as_array)
            .filter(|points| !points.is_empty())
            .ok_or_else(|| invalid("design fit needs 1 to 32 explicit points of measure"))?;
        if points.len() > MAX_POINTS {
            return Err(budget("design fit exceeds 32 points of measure"));
        }
        for field in ["seam_tol_mm", "truing_tol_deg"] {
            optional_number(&self.fit_spec, field, true)?;
        }
        if let Some(truing) = self.fit_spec.get("truing") {
            let checks = truing
                .as_array()
                .ok_or_else(|| invalid("design fit truing must be an array"))?;
            if checks.len() > MAX_SELECTORS {
                return Err(budget("design fit exceeds 64 explicit truing checks"));
            }
            for check in checks {
                nonempty(check, "seam")?;
                if !matches!(nonempty(check, "end")?, "start" | "end") {
                    return Err(invalid("design fit truing end must be start or end"));
                }
            }
        }
        let mut ids = BTreeSet::new();
        for point in points {
            validate_point(point, &mut ids)?;
        }
        Ok(())
    }

    /// Measurement selector work to reserve per altered candidate, after [`Self::validate`].
    /// Each girth instance and path edge component counts separately; width/vertical count once.
    /// The caller also reserves canonical seam/truing and sampled-outline work for the pattern.
    #[must_use]
    pub fn measurement_work_units(&self) -> usize {
        self.fit_spec
            .get("points")
            .and_then(Value::as_array)
            .map_or(0, |points| {
                points.iter().fold(0usize, |total, point| {
                    let units = match point.get("kind").and_then(Value::as_str) {
                        Some("girth") => point
                            .get("instances")
                            .and_then(Value::as_array)
                            .map_or(0, Vec::len),
                        Some("path") => {
                            point
                                .get("refs")
                                .and_then(Value::as_array)
                                .map_or(0, |refs| {
                                    refs.iter().fold(0usize, |count, reference| {
                                        count.saturating_add(
                                            reference.as_str().map_or(0, |s| s.split('+').count()),
                                        )
                                    })
                                })
                        }
                        _ => 1,
                    };
                    total.saturating_add(units)
                })
            })
    }

    /// Evaluate against the actual selected-body context and retain the canonical report intact.
    /// Missing body readings remain `UNMEASURABLE`; out-of-band rows remain available failures.
    #[must_use]
    pub fn evaluate(&self, pattern: &Pattern, context: Option<&Context<'_>>) -> Outcome<Report> {
        Outcome::from_result((|| {
            self.validate()?;
            let context = context.ok_or_else(|| {
                invalid("design fit needs the selected scenario's fitted-body measurement context")
                    .repair(
                        "Evaluate this design request with a model-backed scenario entry point.",
                    )
            })?;
            context.validate()?;
            let validation = validate::validate(pattern, &self.fit_spec, context.body);
            let passed = validation.status() == "CONVERGED";
            let validation = serde_json::to_value(validation).map_err(|error| {
                invalid(format!("cannot serialize canonical design fit: {error}"))
            })?;
            Ok(Report {
                body_identity: context.identity.clone(),
                fit_spec: self.fit_spec.clone(),
                validation,
                passed,
            })
        })())
    }
}

#[cfg(test)]
mod tests {
    use ftx_core::RefusalCode;
    use ftx_pattern::instance::PyError;
    use serde_json::json;

    use super::*;

    const PATTERN: &str =
        include_str!("../../../../../tests/fixtures/materials/synthetic-sewing-pattern.json");

    struct SyntheticBody {
        length_mm: f64,
    }

    impl BodyMeasure for SyntheticBody {
        fn value(&self, name: &str) -> Result<f64, PyError> {
            match name {
                "chest" => Ok(30.0),
                "length" => Ok(self.length_mm),
                _ => Err(PyError::Key(name.into())),
            }
        }

        fn sigma(&self, _name: &str) -> Option<f64> {
            None
        }

        fn landmark_h(&self, part: &str, name: &str) -> Result<f64, PyError> {
            match (part, name) {
                ("torso", "waist") => Ok(0.0),
                ("torso", "chest") => Ok(50.0),
                _ => Err(PyError::Key(format!("{part}/{name}"))),
            }
        }
    }

    fn request() -> Request {
        Request {
            fit_spec: json!({"schema":"fabric-fit/1","points":[{
                "id":"length","kind":"vertical","instance":"back","from":"a","to":"d",
                "body":"length","target_mm":[9.0,10.0,11.0]
            }]}),
        }
    }

    fn report(request: &Request, pattern: &Pattern, body: &SyntheticBody) -> Report {
        let identity = json!({"source":"synthetic-body-fixture","shape_sha256":"a".repeat(64)});
        match request.evaluate(
            pattern,
            Some(&Context {
                body,
                identity: &identity,
            }),
        ) {
            Outcome::Available { result } => {
                assert_eq!(result.body_identity, identity);
                result
            }
            other => panic!("expected canonical report, got {other:?}"),
        }
    }

    #[test]
    fn all_point_kinds_use_the_supplied_body_and_preserve_canonical_report() {
        let pattern = Pattern::from_json(PATTERN).unwrap();
        let original = pattern.raw().clone();
        let mut input = request();
        input.fit_spec["points"].as_array_mut().unwrap().extend([
            json!({"id":"chest","kind":"girth","instances":["front","back"],
                "landmark":"chest","body":"chest","target_mm":[9,10,11]}),
            json!({"id":"width","kind":"width","instance":"front","at_vertex":"a",
                "dy":50,"body":"chest","body_scale":0.5,"target_mm":[4,5,6]}),
            json!({"id":"path","kind":"path","refs":["front/top","back/top"],"scale":0.5,
                "body":"chest","body_scale":0.5,"body_offset_mm":2,"target_mm":[3,3,3]}),
        ]);
        let body = SyntheticBody { length_mm: 90.0 };
        let result = report(&input, &pattern, &body);
        assert!(result.passed);
        assert_eq!(result.fit_spec, input.fit_spec);
        assert_eq!(
            result.validation,
            serde_json::to_value(validate::validate(&pattern, &input.fit_spec, &body)).unwrap()
        );
        assert_eq!(result.validation["points"][0]["finished_mm"], 100.0);
        assert_eq!(result.validation["points"][1]["finished_mm"], 40.0);
        assert_eq!(result.validation["points"][2]["finished_mm"], 20.0);
        assert_eq!(result.validation["points"][3]["ease_mm"], 3.0);
        assert_eq!(input.measurement_work_units(), 6);
        assert_eq!(pattern.raw(), &original);
    }

    #[test]
    fn out_of_band_status_stays_failed_when_residual_rounds_to_zero() {
        let pattern = Pattern::from_json(PATTERN).unwrap();
        let mut input = request();
        input.fit_spec["points"][0]["target_mm"] = json!([0, 0, 0]);
        for (length_mm, status) in [(99.99, "HIGH"), (100.01, "LOW")] {
            let result = report(&input, &pattern, &SyntheticBody { length_mm });
            assert!(!result.passed);
            assert_eq!(result.validation["status"], "NOT_CONVERGED");
            assert_eq!(result.validation["points"][0]["status"], status);
            assert_eq!(result.validation["points"][0]["residual_mm"], 0.0);
        }
    }

    #[test]
    fn unavailable_body_readings_stay_unmeasurable_and_no_context_refuses() {
        let pattern = Pattern::from_json(PATTERN).unwrap();
        assert!(
            matches!(request().evaluate(&pattern, None), Outcome::Refused { refusal }
            if refusal.code == RefusalCode::InvalidDocument && refusal.message.contains("fitted-body"))
        );
        let body = SyntheticBody { length_mm: 90.0 };
        let mut input = request();
        input.fit_spec["points"][0]["body"] = json!("unavailable_tape");
        let missing = report(&input, &pattern, &body);
        assert!(!missing.passed);
        assert_eq!(missing.validation["status"], "BLOCKED");
        assert_eq!(
            missing.validation["missing_measurements"],
            json!(["unavailable_tape"])
        );
        assert_eq!(missing.validation["points"][0]["status"], "UNMEASURABLE");
        assert!(missing.validation["points"][0]["body_mm"].is_null());
        input.fit_spec["points"][0] = json!({"id":"section","kind":"width","instance":"front",
            "landmark":"unavailable_landmark","body":"chest","target_mm":[0,0,0]});
        let missing = report(&input, &pattern, &body);
        assert!(!missing.passed);
        assert_eq!(missing.validation["status"], "NOT_CONVERGED");
        assert_eq!(missing.validation["points"][0]["status"], "UNMEASURABLE");
        assert!(missing.validation["points"][0]["finished_mm"].is_null());
    }

    #[test]
    fn missing_and_oversized_body_identities_cannot_produce_fit_evidence() {
        let pattern = Pattern::from_json(PATTERN).unwrap();
        let body = SyntheticBody { length_mm: 90.0 };
        for identity in [Value::Null, json!({}), json!([]), json!("body")] {
            assert!(matches!(request().evaluate(&pattern, Some(&Context {
                body: &body, identity: &identity,
            })), Outcome::Refused { refusal } if refusal.code == RefusalCode::InvalidDocument));
        }
        let identity = json!({"source":"x".repeat(MAX_IDENTITY_BYTES)});
        assert!(matches!(request().evaluate(&pattern, Some(&Context {
            body: &body, identity: &identity,
        })), Outcome::Refused { refusal } if refusal.code == RefusalCode::BudgetExceeded));
    }

    #[test]
    fn point_success_cannot_bypass_seams_truing_or_completeness() {
        let source: Value = serde_json::from_str(PATTERN).unwrap();
        let body = SyntheticBody { length_mm: 90.0 };
        let mut seam = source.clone();
        seam["pieces"][0]["vertices"]["c"] = json!([20, 120]);
        seam["pieces"][0]["vertices"]["d"] = json!([0, 120]);
        let failed = report(&request(), &Pattern::new(seam), &body);
        assert!(!failed.passed);
        assert_eq!(failed.validation["summary"]["seams_failing"], 1);
        assert_eq!(failed.validation["points"][0]["status"], "IN_BAND");
        let mut truing = source.clone();
        truing["pieces"][0]["vertices"]["a"] = json!([0, 10]);
        let failed = report(&request(), &Pattern::new(truing), &body);
        assert!(!failed.passed);
        assert!(
            failed.validation["summary"]["truing_failing"]
                .as_u64()
                .unwrap()
                > 0
        );
        assert_eq!(failed.validation["points"][0]["status"], "IN_BAND");
        let mut incomplete = source;
        incomplete["pieces"][0]
            .as_object_mut()
            .unwrap()
            .remove("grain");
        let failed = report(&request(), &Pattern::new(incomplete), &body);
        assert!(!failed.passed);
        assert_eq!(
            failed.validation["completeness"],
            json!(["front: no grainline"])
        );
        assert_eq!(failed.validation["points"][0]["status"], "IN_BAND");
    }

    #[test]
    fn malformed_and_oversized_specs_refuse_before_body_evaluation() {
        let base = request();
        for (pointer, value) in [
            ("/schema", json!("fabric-fit/2")),
            ("/schema", Value::Null),
            ("/points", json!([])),
            ("/points/0/id", json!(" ")),
            ("/points/0/body", json!("")),
            ("/points/0/kind", json!("area")),
            ("/points/0/target_mm", json!([0, 1])),
            ("/points/0/target_mm", json!([0, 2, 1])),
            ("/points/0/target_mm", json!([0, -1, 2])),
            ("/points/0/target_mm", json!([0, null, 2])),
            ("/points/0/target_mm", json!([0, "1", 2])),
            ("/points/0/instance", Value::Null),
            ("/points/0/from", json!("")),
        ] {
            let mut input = base.clone();
            *input.fit_spec.pointer_mut(pointer).unwrap() = value;
            assert!(input.validate().is_err(), "accepted invalid {pointer}");
        }
        for field in ["dy", "scale", "body_scale", "body_offset_mm"] {
            let mut input = base.clone();
            input.fit_spec["points"][0][field] = Value::Null;
            assert!(input.validate().is_err(), "accepted null {field}");
        }
        for field in ["seam_tol_mm", "truing_tol_deg"] {
            let mut input = base.clone();
            input.fit_spec[field] = json!(-1);
            assert!(input.validate().is_err(), "accepted negative {field}");
        }
        let mut duplicate = base.clone();
        duplicate.fit_spec["points"] =
            json!([base.fit_spec["points"][0], base.fit_spec["points"][0]]);
        assert!(duplicate.validate().is_err());
        let mut oversized = base.clone();
        oversized.fit_spec["points"] =
            json!(vec![base.fit_spec["points"][0].clone(); MAX_POINTS + 1]);
        assert_eq!(
            oversized.validate().unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        oversized = base.clone();
        oversized.fit_spec["note"] = json!("x".repeat(MAX_SPEC_BYTES));
        assert_eq!(
            oversized.validate().unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        let mut no_schema = base;
        no_schema.fit_spec.as_object_mut().unwrap().remove("schema");
        no_schema.validate().unwrap();
        assert!(
            Request {
                fit_spec: Value::Null
            }
            .validate()
            .is_err()
        );
    }

    #[test]
    fn selector_and_truing_syntax_is_explicit_and_measurement_work_counts_path_components() {
        for point in [
            json!({"kind":"girth","instances":[],"landmark":"chest"}),
            json!({"kind":"girth","instances":["front",null],"landmark":"chest"}),
            json!({"kind":"girth","instances":["front"]}),
            json!({"kind":"width","instance":"front"}),
            json!({"kind":"width","instance":"front","at_vertex":null}),
            json!({"kind":"vertical","instance":"front","from":"a"}),
            json!({"kind":"path","refs":[]}),
            json!({"kind":"path","refs":[""]}),
        ] {
            let mut input = request();
            let mut point = point;
            point["id"] = json!("explicit");
            point["body"] = json!("chest");
            point["target_mm"] = json!([0, 0, 0]);
            input.fit_spec["points"] = json!([point]);
            assert!(input.validate().is_err());
        }
        for value in [
            Value::Null,
            json!([{"seam":"side","end":"middle"}]),
            json!([{"seam":"","end":"start"}]),
        ] {
            let mut input = request();
            input.fit_spec["truing"] = value;
            assert!(input.validate().is_err());
        }
        let mut input = request();
        input.fit_spec["points"] = json!([{"id":"path","kind":"path","body":"chest",
            "refs":["front/bottom+right","back/top"],"target_mm":[0,0,0]}]);
        input.fit_spec["truing"] = json!([{"seam":"side","end":"start"}]);
        input.validate().unwrap();
        assert_eq!(input.measurement_work_units(), 3);
        input.fit_spec["truing"] = json!(vec![
            json!({"seam":"side","end":"start"});
            MAX_SELECTORS + 1
        ]);
        assert_eq!(
            input.validate().unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        input.fit_spec["truing"] = json!([]);
        input.fit_spec["points"][0]["refs"] = json!(vec!["front/top"; MAX_SELECTORS + 1]);
        assert_eq!(
            input.validate().unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
    }
}
