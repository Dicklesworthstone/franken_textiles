//! Measured or explicitly accepted-body sizing through the existing branched garment/export path.

use std::path::{Path, PathBuf};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_drape::drafting::PreparedPatternBody;
use ftx_export::knit::export_knit_document;
use ftx_pattern::Measurements;
use ftx_pattern::knit::{
    BodyPulloverInput, BodySizingBasis, DrapeUncertaintyReceipt, KnitSwatchQualification,
    PULLOVER_BODY_MEASUREMENTS, draft_pullover_from_body,
};
use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::{Args, bad_input, known_options, required, reserve_output, workspace};

fn text(path: &str, limit: u64, label: &str) -> Result<String, Refusal> {
    String::from_utf8(crate::read_regular_bounded(Path::new(path), limit, label)?)
        .map_err(|error| bad_input(format!("{label}: {error}")))
}

fn parse<T: DeserializeOwned>(text: &str, label: &str) -> Result<T, Refusal> {
    serde_json::from_str(text).map_err(|error| bad_input(format!("{label}: {error}")))
}

fn optional_text(args: &Args, key: &str) -> Result<Option<String>, Refusal> {
    args.options
        .get(key)
        .map(|path| text(path, 16 * 1024 * 1024, key))
        .transpose()
}

/// Dispatch target for `ftx knit measured`; completes source admission before creating output.
pub(super) fn cmd_knit_measured(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit measured",
        &[
            "input",
            "measurements",
            "body-fit",
            "model",
            "qual",
            "material-sha256",
            "drape-receipt",
            "created",
            "out",
        ],
    )?;
    super::no_extra_positional(
        args,
        2,
        "ftx knit measured --input DESIGN.json --measurements MEAS.json [--body-fit ACCEPTED.json --model FILE] --out NEW_DIR",
    )?;
    if args.options.contains_key("model") != args.options.contains_key("body-fit") {
        return Err(bad_input(
            "--model and --body-fit must be supplied together; measured knitting never starts a new body fit",
        ));
    }
    let out = PathBuf::from(required(args, "out")?);
    let input_text = text(required(args, "input")?, 256 * 1024, "measured knit design")?;
    let input: BodyPulloverInput = parse(&input_text, "measured knit design")?;
    let measurements_text = text(
        required(args, "measurements")?,
        16 * 1024 * 1024,
        "measurements",
    )?;
    let original: Value = parse(&measurements_text, "measurements")?;
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
    let profile = super::parse_machine(&input.machine)?;
    let qualification_text = optional_text(args, "qual")?;
    let receipt_text = optional_text(args, "drape-receipt")?;
    let qualification: Option<KnitSwatchQualification> = qualification_text
        .as_deref()
        .map(|value| parse(value, "swatch qualification"))
        .transpose()?;
    let receipt: Option<DrapeUncertaintyReceipt> = receipt_text
        .as_deref()
        .map(|value| parse(value, "drape receipt"))
        .transpose()?;
    let material_sha256 = super::expected_knit_material(args)?;
    let created = args
        .options
        .get("created")
        .cloned()
        .map_or_else(crate::export::utc_minute, Ok)?;
    let as_of = created
        .get(..10)
        .ok_or_else(|| bad_input("created must include an ISO date"))?;
    let readiness_options = ftx_measure::ReadinessOptions {
        use_priors: input.use_priors,
        closed_cuff: true,
        ..Default::default()
    };
    let readiness = json!({
        "bodice":ftx_measure::readiness(&original,"bodice",&readiness_options,as_of)?,
        "closed_cuff_sleeve":ftx_measure::readiness(&original,"set_in_sleeve",&readiness_options,as_of)?,
    });
    let mut body_fit_text = None;
    let (mut draft, mut source) = if args.options.contains_key("body-fit") {
        let (model, model_sha256, bytes) = crate::body_source::accepted_inputs(args)?;
        let report = String::from_utf8(bytes).map_err(|error| bad_input(error.to_string()))?;
        let body =
            PreparedPatternBody::prepare(&model, &model_sha256, &measurements, Some(&report), &[])?;
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
        body_fit_text = Some(report);
        (draft, source)
    } else {
        let mut sources = Map::new();
        for name in PULLOVER_BODY_MEASUREMENTS {
            let entry = &measurements["fused"][name];
            if entry["flags"]
                .as_array()
                .is_some_and(|flags| flags.iter().any(|flag| flag == "REMEASURE"))
            {
                return Err(bad_input(format!(
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
            json!({"kind":"measurement_evidence", "body_values":sources,
            "body_values_basis":"FUSED_MEASUREMENTS", "body_refitted":false}),
        )
    };
    let total_source = input_text.len()
        + measurements_text.len()
        + body_fit_text.as_ref().map_or(0, String::len)
        + qualification_text.as_ref().map_or(0, String::len)
        + receipt_text.as_ref().map_or(0, String::len);
    if total_source > 32 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit source documents exceed 32 MiB",
        ));
    }
    source["measurements_sha256"] = json!(sha256_hex(measurements_text.as_bytes()));
    source["input_sha256"] = json!(sha256_hex(input_text.as_bytes()));
    source["use_priors"] = json!(input.use_priors);
    source["measurement_readiness"] = readiness;
    draft.doc.metadata.insert(
        "body_sizing_source".into(),
        serde_json::to_string(&source).map_err(|error| bad_input(error.to_string()))?,
    );
    let mut bundle = export_knit_document(
        &draft.doc,
        &profile,
        qualification.as_ref(),
        material_sha256,
        receipt.as_ref(),
        &created,
    )?;
    let report = json!({"source":source, "sizing":draft.report,
        "measurement_readiness_established":false, "fit_claimed":false});
    bundle
        .files
        .insert("body_draft.json".into(), workspace::json_bytes(&report)?);
    bundle.files.insert(
        "measurements.fused.json".into(),
        workspace::json_bytes(&measurements)?,
    );
    bundle
        .files
        .insert("input.source.json".into(), input_text.into_bytes());
    bundle.files.insert(
        "measurements.source.json".into(),
        measurements_text.into_bytes(),
    );
    if let Some(report) = body_fit_text {
        bundle
            .files
            .insert("body_fit.source.json".into(), report.into_bytes());
    }
    if let Some(qualification) = qualification_text {
        bundle.files.insert(
            "qualification.source.json".into(),
            qualification.into_bytes(),
        );
    }
    if let Some(receipt) = receipt_text {
        bundle
            .files
            .insert("drape_receipt.source.json".into(), receipt.into_bytes());
    }
    if bundle.files.values().map(Vec::len).sum::<usize>() > 64 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "measured knit artifacts exceed 64 MiB",
        ));
    }
    let files: Vec<_> = bundle
        .files
        .iter()
        .map(|(name, bytes)| {
            json!({
                "name":name,"bytes":bytes.len(),"sha256":sha256_hex(bytes),
            })
        })
        .collect();
    let result = json!({
        "machine":profile.id, "pattern_sha256":draft.doc.canonical_sha256()?,
        "drafting":report["sizing"], "body_source":report["source"],
        "files":files, "vm_report":bundle.compiled.vm_report,
        "cert":bundle.compiled.cert, "backend_export_refusal":bundle.backend_export_refusal,
        "kcode_verification":bundle.kcode_verification, "out_dir":out,
        "machine_executable":false,"hardware_execution_authorized":false,
        "physical_knitting_validated":false,"fit_claimed":false,
    });
    reserve_output(&out, "knit measured")?;
    for (name, bytes) in &bundle.files {
        workspace::write_new(&out.join(name), bytes)?;
    }
    Ok((
        result,
        0,
        format!(
            "Drafted and verified body-sized three-branch pullover for {} -> {}. Body/design sources and exact gauge residuals are retained; physical fit remains unqualified.",
            profile.id,
            out.display()
        ),
    ))
}
