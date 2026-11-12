//! Initial patterns from the same accepted body used by garment studies and draping.
//! Model dimensions stay predictions; hand/foot passage remains separate supplied evidence.

use ftx_body::AnnyModel;
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_pattern::{
    BodyMeasure, Pattern,
    draft::{DraftBody, PROGRAMS, form::Meas},
    instance::PyError,
};
use serde_json::{Map, Value, json};

use crate::study::fitted::{BodyRequest, FittedBody, FlatFitBody};

/// A model-derived draft and its separate body/flat-validation evidence.
pub struct Draft {
    /// Ordinary `fabric-pattern/1`, with the selected drafting body's identity in metadata.
    pub pattern: Value,
    /// Ordinary `fabric-fit/1` from the existing drafting program.
    pub fit_spec: Value,
    /// Selected shape, model readings and the original supplemental passage values.
    pub body: Value,
    /// Flat validation on the same body that supplied drafting dimensions.
    pub validation: Value,
    /// Human-readable rendering of the same selected-body flat validation.
    pub validation_table: String,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn passage_evidence(measurements: &Value, name: &str) -> Result<Value, Refusal> {
    let value = Meas(measurements).get(name).map_err(|_| {
        invalid(format!(
            "selected-body drafting needs an explicit '{name}' passage value; the body model does not infer it"
        ))
    })?;
    if !value.is_finite() || value <= 0.0 {
        return Err(invalid(format!("'{name}' must be finite and positive")));
    }
    let source = measurements
        .get("fused")
        .unwrap_or(measurements)
        .get(name)
        .ok_or_else(|| invalid(format!("missing '{name}' source")))?;
    if source["flags"]
        .as_array()
        .is_some_and(|flags| flags.iter().any(|flag| flag == "PRIOR_ONLY"))
        || source["primary_provenance"] == "MODEL_FIT"
        || source["sources"].as_array().is_some_and(|sources| {
            sources
                .iter()
                .any(|source| source["used"] == true && source["provenance"] == "MODEL_FIT")
        })
    {
        return Err(invalid(format!(
            "'{name}' needs separately supplied passage evidence; automatic chart priors and model predictions cannot fill it"
        )));
    }
    Ok(json!({"value_mm":value,"source":source}))
}

struct DraftingBody<'a> {
    reference: &'a FlatFitBody,
    passage: Map<String, Value>,
}

impl DraftBody for DraftingBody<'_> {
    fn get(&self, name: &str) -> Result<f64, Refusal> {
        if matches!(name, "hand_pass" | "foot_pass") {
            return self
                .passage
                .get(name)
                .and_then(|value| value["value_mm"].as_f64())
                .ok_or_else(|| invalid(format!("supply an explicit '{name}' passage measurement")));
        }
        DraftBody::get(self.reference, name)
    }

    fn landmark_h(&self, part: &str, name: &str) -> Result<f64, Refusal> {
        DraftBody::landmark_h(self.reference, part, name)
    }

    fn frame_origin(&self, part: &str) -> Result<Vec3, Refusal> {
        DraftBody::frame_origin(self.reference, part)
    }

    fn section_radius(&self, part: &str, h_mm: f64) -> Result<f64, Refusal> {
        DraftBody::section_radius(self.reference, part, h_mm)
    }
}

impl BodyMeasure for DraftingBody<'_> {
    fn value(&self, name: &str) -> Result<f64, PyError> {
        DraftBody::get(self, name).map_err(|_| PyError::Key(name.into()))
    }

    fn sigma(&self, name: &str) -> Option<f64> {
        self.passage.get(name)?["source"]["sigma_mm"]
            .as_f64()
            .filter(|sigma| sigma.is_finite() && *sigma >= 0.0)
    }

    fn landmark_h(&self, part: &str, name: &str) -> Result<f64, PyError> {
        BodyMeasure::landmark_h(self.reference, part, name)
    }
}

/// Draft from an intact accepted image-fit report, without refitting or promoting measurements.
/// The caller hashes and verifies the same model bytes before constructing `model`.
/// Geometry and dimensions use the selected shape in the canonical 35-degree A-pose. Only the
/// program's hand/foot passage value comes from `measurements`; missing model sections refuse.
///
/// # Errors
/// Invalid source report/model/demographics, absent required passage evidence, unavailable body
/// geometry, invalid style, or a drafting solve that cannot satisfy its geometric constraints.
pub fn draft_on_model(
    model: &AnnyModel,
    model_sha256: &str,
    program: &str,
    measurements: &Value,
    accepted_report_text: &str,
    style: Option<&Value>,
) -> Result<Draft, Refusal> {
    if !PROGRAMS.contains(&program) {
        return Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown program {program:?}"),
        ));
    }
    if accepted_report_text.len() > 16 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "accepted body-fit report exceeds 16 MiB",
        ));
    }
    let accepted: Value = serde_json::from_str(accepted_report_text)
        .map_err(|e| invalid(format!("body-fit report: {e}")))?;
    let request = BodyRequest::read(&json!({
        "schema":"ftx-scenario-body/1", "measurements":measurements,
        "accepted_body_fit":accepted, "arm_abduction_deg":35.0
    }))?;
    let selected = FittedBody::prepare(model, model_sha256, &request)?;
    let reference = selected.flat_fit_body(model, model_sha256)?;
    let mut passage = Map::new();
    let passage_name = match program {
        "zouave_bolero" => Some("hand_pass"),
        "zouave_trousers" => Some("foot_pass"),
        _ => None,
    };
    if let Some(name) = passage_name {
        passage.insert(name.into(), passage_evidence(measurements, name)?);
    }
    let body = DraftingBody {
        reference: &reference,
        passage,
    };
    let (mut pattern, fit_spec) = ftx_pattern::draft::draft_on_body(program, &body, style)?;
    let identity = json!({"reference":reference.identity(),
        "source_report_sha256":sha256_hex(accepted_report_text.as_bytes()),
        "supplemental_measurements":body.passage});
    pattern["meta"]["draft_body"] = identity.clone();
    let report = ftx_pattern::validate(&Pattern::new(pattern.clone()), &fit_spec, &body);
    let validation_table = ftx_pattern::format_table(&report);
    let validation = serde_json::to_value(report).map_err(|e| invalid(e.to_string()))?;
    let mut values = Map::new();
    for name in reference.identity()["available_measurements"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .chain(std::iter::once("body_rise"))
    {
        if let Ok(value) = DraftBody::get(&reference, name) {
            values.insert(name.into(), json!(value));
        }
    }
    Ok(Draft {
        pattern,
        fit_spec,
        validation,
        validation_table,
        body: json!({"schema":"ftx-body-draft/1", "provenance":"MODEL_FIT",
            "identity":identity, "model_readings_mm":values, "body_fit":selected.report,
            "body_rise_basis":"Reference-pose vertical waist-to-crotch distance; no inferred seated allowance. The program's authored rise_ease remains separate.",
            "no_claim":"Pattern dimensions and placement come from the selected model in its reference A-pose. Passage values retain their supplied evidence. Model predictions do not change measurements or readiness, supply uncertainty, or establish physical garment fit."}),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passage_requires_separate_evidence_and_preserves_its_original_basis() {
        let explicit = json!({"hand_pass":155.0});
        assert_eq!(
            passage_evidence(&explicit, "hand_pass").unwrap()["source"],
            155.0
        );
        let record = json!({"value_mm":160.0,"sigma_mm":3.0,
            "primary_provenance":"MEASURED_TAPE","flags":[],
            "sources":[{"provenance":"MEASURED_TAPE","used":true}]});
        let mut evidence = json!({"fused":{"hand_pass":record}});
        assert_eq!(
            passage_evidence(&evidence, "hand_pass").unwrap()["source"],
            record
        );
        evidence["fused"]["hand_pass"]["flags"] = json!(["PRIOR_ONLY"]);
        assert!(passage_evidence(&evidence, "hand_pass").is_err());
        evidence["fused"]["hand_pass"]["flags"] = json!([]);
        evidence["fused"]["hand_pass"]["sources"][0]["provenance"] = json!("MODEL_FIT");
        assert!(passage_evidence(&evidence, "hand_pass").is_err());
        assert!(passage_evidence(&json!({}), "foot_pass").is_err());
        assert!(passage_evidence(&json!({"foot_pass":0.0}), "foot_pass").is_err());
    }
}
