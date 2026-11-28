//! Local measured or accepted-body pullover drafting through ordinary qualification/export.

use ftx_body::{AnnyData, AnnyModel};
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_drape::drafting::PreparedPatternBody;
use ftx_export::knit::export_knit_document;
use ftx_pattern::Measurements;
use ftx_pattern::knit::{
    BodyPulloverInput, BodySizingBasis, DrapeUncertaintyReceipt, KnitSwatchQualification,
    PULLOVER_BODY_MEASUREMENTS, draft_pullover_from_body,
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Map, Value, json};

/// Original source documents remain raw strings across the JavaScript boundary.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitBodyRequest {
    /// Complete design/machine/gauge/explicit-prior-policy input file.
    pub input_json: String,
    /// Original `fabric-measurements/1` records; stored fusion caches are recomputed.
    pub measurements_json: String,
    /// Optional accepted Anny report; requires matching model bytes as the other argument.
    pub body_fit_json: Option<String>,
    /// Optional exact measured swatch qualification.
    pub qualification_json: Option<String>,
    /// Independently selected material identity; never inferred from the qualification itself.
    pub material_sha256: Option<String>,
    /// Optional exact sealed drape report for the resulting document.
    pub drape_receipt_json: Option<String>,
    /// Explicit deterministic export date; shares the existing saved-knit default.
    #[serde(default = "crate::knit::default_created")]
    pub created: String,
}

fn invalid(message: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, message)
}

fn parse<T: DeserializeOwned>(text: &str, label: &str) -> Result<T, Refusal> {
    serde_json::from_str(text).map_err(|error| invalid(format!("{label}: {error}")))
}

fn json_bytes(value: &Value) -> Result<Vec<u8>, Refusal> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| invalid(error.to_string()))?;
    bytes.push(b'\n');
    Ok(bytes)
}

/// Draft and export a complete three-branch pullover from measurement or accepted-model girths.
/// Empty model bytes select original measurement evidence. A supplied model and accepted report
/// must be paired; their ordinary source-bound authority is reused without a new tape fit.
///
/// # Errors
/// Bounded-input, source/admission, age, missing body dimension, design, qualification, discrete
/// compiler, chart/export and output-budget refusals. No partial download is returned.
pub fn knit_body_json(request_json: &str, model_bytes: &[u8]) -> Result<String, Refusal> {
    if request_json.len() > 32 * 1024 * 1024 || model_bytes.len() > 512 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit request/model exceeds its 32/512 MiB budget",
        ));
    }
    let req: KnitBodyRequest = parse(request_json, "measured knit request")?;
    if req.input_json.len() > 256 * 1024
        || req.measurements_json.len() > 16 * 1024 * 1024
        || [
            &req.body_fit_json,
            &req.qualification_json,
            &req.drape_receipt_json,
        ]
        .iter()
        .any(|text| {
            text.as_ref()
                .is_some_and(|text| text.len() > 16 * 1024 * 1024)
        })
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit design or source exceeds its 256 KiB/16 MiB document budget",
        ));
    }
    if req.body_fit_json.is_some() == model_bytes.is_empty() {
        return Err(invalid(
            "model bytes and body_fit_json must be supplied together; measured knitting never starts a new body fit",
        ));
    }
    let input: BodyPulloverInput = parse(&req.input_json, "measured knit design")?;
    let original: Value = parse(&req.measurements_json, "measurements")?;
    if original["records"]
        .as_array()
        .is_some_and(|records| records.len() > 4096)
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit source exceeds 4096 measurement records",
        ));
    }
    let measurements = ftx_measure::fuse(&original, input.use_priors)?;
    input
        .design
        .check_wearer_age(measurements["wearer"]["age_months"].as_f64())?;
    let profile = crate::knit::resolve_knit_machine(&input.machine)?;
    let qualification: Option<KnitSwatchQualification> = req
        .qualification_json
        .as_deref()
        .map(|text| parse(text, "swatch qualification"))
        .transpose()?;
    let receipt: Option<DrapeUncertaintyReceipt> = req
        .drape_receipt_json
        .as_deref()
        .map(|text| parse(text, "drape receipt"))
        .transpose()?;
    if req
        .material_sha256
        .as_ref()
        .is_some_and(|value| value.trim().is_empty())
    {
        return Err(invalid(
            "material_sha256 must identify the independently selected source material",
        ));
    }
    let as_of = req
        .created
        .get(..10)
        .ok_or_else(|| invalid("created must include an ISO date"))?;
    let readiness_options = ftx_measure::ReadinessOptions {
        use_priors: input.use_priors,
        closed_cuff: true,
        ..Default::default()
    };
    let readiness = json!({
        "bodice":ftx_measure::readiness(&original,"bodice",&readiness_options,as_of)?,
        "closed_cuff_sleeve":ftx_measure::readiness(&original,"set_in_sleeve",&readiness_options,as_of)?,
    });
    let (mut draft, mut source) = if let Some(report) = &req.body_fit_json {
        let model_sha256 = sha256_hex(model_bytes);
        let model = AnnyModel::from_data(AnnyData::from_bytes(model_bytes, true)?)?;
        let body =
            PreparedPatternBody::prepare(&model, &model_sha256, &measurements, Some(report), &[])?;
        let evidence = body.evidence();
        input.design.check_wearer_age(
            evidence["body_fit"]["source_fit"]["anny_configuration"]["age_years"]
                .as_f64()
                .map(|years| years * 12.0),
        )?;
        let draft = draft_pullover_from_body(
            &body,
            BodySizingBasis::ModelFit,
            &input.design,
            &input.gauge,
            &profile,
        )?;
        let source = json!({
            "kind":"accepted_body", "model_sha256":model_sha256,
            "body_fit_sha256":sha256_hex(report.as_bytes()), "identity":body.identity(),
            "body_values_basis":"MODEL_FIT", "body_refitted":false,
            "measurement_comparisons":evidence["body_fit"]["residuals"],
        });
        (draft, source)
    } else {
        let mut sources = Map::new();
        for name in PULLOVER_BODY_MEASUREMENTS {
            let entry = &measurements["fused"][name];
            if entry["flags"]
                .as_array()
                .is_some_and(|flags| flags.iter().any(|flag| flag == "REMEASURE"))
            {
                return Err(invalid(format!(
                    "'{name}' has conflicting measurement evidence; remeasure before knitting a body-sized garment"
                )));
            }
            let status = readiness
                .as_object()
                .into_iter()
                .flat_map(|map| map.values())
                .filter_map(|report| report["items"].as_array())
                .flatten()
                .find(|item| item["name"] == name)
                .cloned()
                .unwrap_or(Value::Null);
            sources.insert(name.into(), json!({"evidence":entry,"readiness":status}));
        }
        let draft = draft_pullover_from_body(
            &Measurements(measurements.clone()),
            BodySizingBasis::FusedMeasurements,
            &input.design,
            &input.gauge,
            &profile,
        )?;
        (
            draft,
            json!({"kind":"measurement_evidence","body_values":sources,
            "body_values_basis":"FUSED_MEASUREMENTS","body_refitted":false}),
        )
    };
    source["measurements_sha256"] = json!(sha256_hex(req.measurements_json.as_bytes()));
    source["input_sha256"] = json!(sha256_hex(req.input_json.as_bytes()));
    source["use_priors"] = json!(input.use_priors);
    source["measurement_readiness"] = readiness;
    draft.doc.metadata.insert(
        "body_sizing_source".into(),
        serde_json::to_string(&source).map_err(|error| invalid(error.to_string()))?,
    );
    let mut bundle = export_knit_document(
        &draft.doc,
        &profile,
        qualification.as_ref(),
        req.material_sha256.as_deref(),
        receipt.as_ref(),
        &req.created,
    )?;
    let report = json!({"source":source,"sizing":draft.report,
        "measurement_readiness_established":false,"fit_claimed":false});
    bundle
        .files
        .insert("body_draft.json".into(), json_bytes(&report)?);
    bundle
        .files
        .insert("measurements.fused.json".into(), json_bytes(&measurements)?);
    bundle
        .files
        .insert("input.source.json".into(), req.input_json.into_bytes());
    bundle.files.insert(
        "measurements.source.json".into(),
        req.measurements_json.into_bytes(),
    );
    if let Some(source) = req.body_fit_json {
        bundle
            .files
            .insert("body_fit.source.json".into(), source.into_bytes());
    }
    if let Some(source) = req.qualification_json {
        bundle
            .files
            .insert("qualification.source.json".into(), source.into_bytes());
    }
    if let Some(source) = req.drape_receipt_json {
        bundle
            .files
            .insert("drape_receipt.source.json".into(), source.into_bytes());
    }
    let pattern_json = std::str::from_utf8(&bundle.files["pattern.knit.json"])
        .map_err(|error| invalid(error.to_string()))?
        .to_owned();
    let files: Vec<_> = bundle.files.into_iter().collect();
    if files.iter().map(|(_, bytes)| bytes.len()).sum::<usize>() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit artifacts exceed 64 MiB",
        ));
    }
    let manifest: Vec<_> = files
        .iter()
        .map(|(name, bytes)| {
            json!({
                "name":name,"bytes":bytes.len(),"sha256":sha256_hex(bytes),
            })
        })
        .collect();
    let archive = crate::zip::zip(&files, &req.created);
    let output = serde_json::to_string(&json!({
        "machine":profile.id,"pattern_sha256":draft.doc.canonical_sha256()?,
        "pattern_json":pattern_json,"files":manifest,
        "archive_base64":crate::knit::encode_base64(&archive),"archive_sha256":sha256_hex(&archive),
        "drafting":report["sizing"],"body_source":report["source"],
        "vm_report":bundle.compiled.vm_report,"cert":bundle.compiled.cert,
        "backend_export_refusal":bundle.backend_export_refusal,
        "kcode_verification":bundle.kcode_verification,
        "machine_executable":false,"hardware_execution_authorized":false,
        "physical_knitting_validated":false,"fit_claimed":false,
    }))
    .map_err(|error| invalid(error.to_string()))?;
    if output.len() > 96 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit response exceeds 96 MiB",
        ));
    }
    Ok(output)
}

/// Browser entry with structured native refusal data, never an unstructured JavaScript exception.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = knit_body_json)]
pub fn web_knit_body_json(request_json: &str, model_bytes: &[u8]) -> String {
    match knit_body_json(request_json, model_bytes) {
        Ok(result) => format!(r#"{{"ok":true,"result":{result}}}"#),
        Err(refusal) => json!({"ok":false,"refusal":refusal}).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> Value {
        let input = json!({
            "machine":"hand-knit","use_priors":false,
            "gauge":{"wales_per_100mm":20.0,"courses_per_100mm":25.0,"needle_pitch_mm":4.0,
                "yarn_tex":180.0,"thickness_mm":2.0,"relaxed_after_wash":true},
            "design":{"name":"synthetic-measured-pullover","wearer_age_class":"adult",
                "chest_ease_mm":21.0,"neck_ease_mm":10.0,"upper_arm_ease_mm":21.0,"wrist_ease_mm":-1.0,
                "yoke_depth_mm":40.0,"body_length_below_arm_mm":80.0,"sleeve_length_below_arm_mm":60.0,
                "hem_rib_courses":2,"collar_rib_courses":2,"cuff_rib_courses":2,
                "cam_setting":13.0,"yarn_id":"synthetic-yarn","body_carrier_id":"2",
                "left_sleeve_carrier_id":"1","right_sleeve_carrier_id":"3","has_hood_or_neck_drawstring":false}
        });
        let records: Vec<_> = [
            ("chest", 180.0),
            ("neck_base", 90.0),
            ("upper_arm", 60.0),
            ("wrist", 40.0),
        ]
        .into_iter()
        .map(|(name, value)| {
            ftx_measure::make_record(
                name,
                value,
                1.0,
                "MEASURED_TAPE",
                "synthetic numerical test only",
                None,
                "",
            )
            .unwrap()
        })
        .collect();
        json!({"input_json":format!("{}\n",serde_json::to_string_pretty(&input).unwrap()),
            "measurements_json":format!("{}\n",json!({"schema":"fabric-measurements/1","wearer":{},"records":records,
                "fused":{"chest":{"value_mm":999.0}}})),
            "created":"2026-11-28T12:00"})
    }

    #[test]
    fn portable_measurement_draft_recomputes_evidence_and_exports_a_complete_garment() {
        let req = request();
        let result: Value =
            serde_json::from_str(&knit_body_json(&req.to_string(), &[]).unwrap()).unwrap();
        assert_eq!(
            result["drafting"]["dimensions"]["chest_girth_mm"]["body_value_mm"],
            180.0
        );
        assert_eq!(
            result["drafting"]["dimensions"]["chest_girth_mm"]["residual_mm"],
            -1.0
        );
        assert_eq!(
            result["body_source"]["measurements_sha256"],
            sha256_hex(req["measurements_json"].as_str().unwrap().as_bytes())
        );
        assert_eq!(
            result["body_source"]["body_values"]["chest"]["evidence"]["primary_provenance"],
            "MEASURED_TAPE"
        );
        assert_eq!(
            result["body_source"]["body_values"]["chest"]["readiness"]["status"],
            "OK"
        );
        assert_eq!(
            result["body_source"]["measurement_readiness"]["bodice"]["status"],
            "NOT_READY"
        );
        for name in [
            "pattern.knit.json",
            "program.k",
            "chart.svg",
            "chart.pdf",
            "input.source.json",
            "measurements.source.json",
            "body_draft.json",
        ] {
            assert!(
                result["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|file| file["name"] == name)
            );
        }
        assert_eq!(result["fit_claimed"], false);
        assert_eq!(result["hardware_execution_authorized"], false);
        let doc: ftx_pattern::knit::KnitDoc =
            serde_json::from_str(result["pattern_json"].as_str().unwrap()).unwrap();
        assert_eq!(result["pattern_sha256"], doc.canonical_sha256().unwrap());
    }

    #[test]
    fn portable_model_pairing_age_missing_dimensions_and_conflicting_evidence_refuse() {
        let original = request();
        assert!(
            knit_body_json(&original.to_string(), b"unexpected model")
                .unwrap_err()
                .message
                .contains("together")
        );
        let mut changed = original.clone();
        changed["body_fit_json"] = json!("{}");
        assert!(
            knit_body_json(&changed.to_string(), &[])
                .unwrap_err()
                .message
                .contains("together")
        );
        let mut measurements: Value =
            serde_json::from_str(original["measurements_json"].as_str().unwrap()).unwrap();
        measurements["wearer"]["age_months"] = json!(30.0);
        changed = original.clone();
        changed["measurements_json"] = json!(measurements.to_string());
        assert_eq!(
            knit_body_json(&changed.to_string(), &[]).unwrap_err().code,
            RefusalCode::MaterialProcessHazard
        );
        measurements["wearer"] = json!({});
        measurements["records"].as_array_mut().unwrap().pop();
        changed["measurements_json"] = json!(measurements.to_string());
        assert!(
            knit_body_json(&changed.to_string(), &[])
                .unwrap_err()
                .message
                .contains("wrist")
        );
        measurements =
            serde_json::from_str(original["measurements_json"].as_str().unwrap()).unwrap();
        measurements["records"].as_array_mut().unwrap().push(
            ftx_measure::make_record(
                "chest",
                300.0,
                1.0,
                "PHOTO_ESTIMATED",
                "independent synthetic method",
                None,
                "",
            )
            .unwrap(),
        );
        changed["measurements_json"] = json!(measurements.to_string());
        assert!(
            knit_body_json(&changed.to_string(), &[])
                .unwrap_err()
                .message
                .contains("conflicting")
        );
    }

    #[test]
    fn chart_prior_keeps_its_provenance_and_readiness_refusal() {
        let mut req = request();
        let mut input: Value = serde_json::from_str(req["input_json"].as_str().unwrap()).unwrap();
        input["use_priors"] = json!(true);
        req["input_json"] = json!(input.to_string());
        let mut measurements: Value =
            serde_json::from_str(req["measurements_json"].as_str().unwrap()).unwrap();
        measurements["records"][0]["provenance"] = json!("CHART_PRIOR");
        req["measurements_json"] = json!(measurements.to_string());
        let result: Value =
            serde_json::from_str(&knit_body_json(&req.to_string(), &[]).unwrap()).unwrap();
        assert_eq!(
            result["body_source"]["body_values"]["chest"]["evidence"]["primary_provenance"],
            "CHART_PRIOR"
        );
        assert_eq!(
            result["body_source"]["body_values"]["chest"]["readiness"]["status"],
            "PRIOR_ONLY"
        );
        assert_eq!(result["body_source"]["use_priors"], true);
        assert_eq!(result["fit_claimed"], false);
    }
}
