//! Caller-authored model correspondences, bound to one native model and source request.

use std::{collections::BTreeSet, path::Path};

use ftx_body::{AnnyData, AnnyModel};
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Adapter, Configuration, Target, review::Bundle};
use crate::{Args, bad_input, known_options, required, workspace};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Annotations {
    schema: String,
    source_input: String,
    source_input_sha256: String,
    model_sha256: String,
    keypoints: std::collections::BTreeMap<String, Target>,
}

fn configuration(input: &Value) -> Result<Configuration, Refusal> {
    if input["schema"] != "ftx-anny-capture-input/1"
        && input["schema"] != "ftx-anny-body-fit-input/1"
    {
        return Err(bad_input(
            "model target authoring needs an existing ftx-anny-capture-input/1 or ftx-anny-body-fit-input/1 declaration",
        ));
    }
    serde_json::from_value(input["model"].clone())
        .map_err(|e| bad_input(format!("invalid Anny model configuration: {e}")))
}

fn observed_names(input: &Value) -> Result<BTreeSet<String>, Refusal> {
    let frames = input["fit"]["frames"]
        .as_array()
        .filter(|f| f.len() <= 8)
        .ok_or_else(|| bad_input("model target input needs at most eight declared frames"))?;
    let mut names = BTreeSet::new();
    for frame in frames {
        let points = frame["keypoints"]
            .as_array()
            .filter(|p| p.len() <= 128)
            .ok_or_else(|| bad_input("each declared frame needs at most 128 keypoints"))?;
        let mut frame_names = BTreeSet::new();
        for point in points {
            let name = point["name"]
                .as_str()
                .filter(|n| !n.trim().is_empty() && n.len() <= 256)
                .ok_or_else(|| {
                    bad_input("observed keypoints need nonempty names up to 256 bytes")
                })?;
            if !frame_names.insert(name) {
                return Err(bad_input("duplicate observed keypoint name within a frame"));
            }
            names.insert(name.to_owned());
        }
    }
    if names.len() > 128 {
        return Err(bad_input(
            "at most 128 distinct observed target names are supported",
        ));
    }
    Ok(names)
}

fn model(path: &str, expected: &str) -> Result<AnnyModel, Refusal> {
    let bytes = super::super::video::read_limited(Path::new(path), 256 * 1024 * 1024)?;
    if sha256_hex(&bytes) != expected {
        return Err(bad_input(
            "model SHA-256 mismatch; no target output was published",
        ));
    }
    AnnyModel::from_data(AnnyData::from_bytes(&bytes, true)?)
}

pub(super) fn review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure body-target-review", &["model", "out"])?;
    let output = Path::new(required(args, "out")?);
    super::review::preflight(output)?;
    let file = super::super::single(args, "body-target-review")?;
    let (input, bytes) = super::super::intrinsics::read_document(file)?;
    let config = configuration(&input)?;
    let names = observed_names(&input)?;
    let source = std::fs::canonicalize(file)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{file}: {e}")))?;
    let annotations = Annotations {
        schema: "ftx-body-targets/1".into(),
        source_input: source
            .to_str()
            .ok_or_else(|| bad_input("source input path must be UTF-8"))?
            .into(),
        source_input_sha256: sha256_hex(&bytes),
        model_sha256: config.model_sha256.clone(),
        keypoints: config.keypoints.clone(),
    };
    let model = model(required(args, "model")?, &config.model_sha256)?;
    let adapter = Adapter::for_target_review(model, config, &annotations.model_sha256)?;
    let geometry = adapter.target_preview()?;
    let preview = &geometry["preview"];
    let editor_data = json!({"annotations":annotations,"observed_names":names,
        "mesh":geometry["mesh"],"bones":geometry["bones"],"preview":preview});
    let mut bundle = Bundle::new();
    let source_entry = bundle.push("source-input.json".into(), bytes)?;
    let annotations_entry = bundle.push(
        "body-targets.json".into(),
        workspace::json_bytes(&json!(annotations))?,
    )?;
    let geometry_entry = bundle.push("model.json".into(), workspace::json_bytes(&geometry)?)?;
    let editor = bundle.push(
        "editor.js".into(),
        include_bytes!("target_review.js").to_vec(),
    )?;
    let page = page(&editor_data)?;
    let viewer = bundle.push("index.html".into(), page.into_bytes())?;
    let result = json!({"schema":"ftx-body-target-review/1","status":"TARGET_REVIEW_REQUIRED",
        "body_model_loaded":true,"body_fitted":false,"input_sha256":annotations.source_input_sha256,
        "model_sha256":annotations.model_sha256,"preview":preview,"observed_names":names,
        "original_input":source_entry,"annotations":annotations_entry,"geometry":geometry_entry,
        "viewer":viewer,"editor":editor,
        "no_claim":"Caller-authored correspondences on an unfitted model. Source cameras and observations were not validated here. No inferred anatomy, observed body, fit, physical accuracy or measurement readiness."});
    bundle.publish_report(output, &result, "review.json")?;
    Ok((
        result,
        0,
        format!(
            "MODEL TARGET REVIEW: UNFITTED\nOpen {}/index.html; apply the downloaded body-targets.json with measure body-targets",
            output.display()
        ),
    ))
}

pub(super) fn rebase_capture(input: &mut Value, source: &Path) -> Result<(), Refusal> {
    if input["schema"] != "ftx-anny-capture-input/1" {
        return Ok(());
    }
    let base = source
        .parent()
        .ok_or_else(|| bad_input("source capture input needs a parent directory"))?;
    let absolute = |value: &mut Value| -> Result<(), Refusal> {
        let name = value
            .as_str()
            .filter(|n| !n.is_empty() && !n.contains('\0'))
            .ok_or_else(|| bad_input("capture file paths must be nonempty strings"))?;
        let path = base.join(name);
        *value = json!(
            path.to_str()
                .ok_or_else(|| bad_input("capture paths must be UTF-8"))?
        );
        Ok(())
    };
    absolute(&mut input["video_manifest"])?;
    if let Some(value) = input.get_mut("silhouettes").filter(|v| !v.is_null()) {
        absolute(value)?;
    }
    for frame in input["fit"]["frames"].as_array_mut().unwrap() {
        absolute(&mut frame["calibration"])?;
        if frame["reference"].is_string() {
            absolute(&mut frame["reference"])?;
        } else {
            let sheet = frame["reference"]
                .as_object_mut()
                .and_then(|r| r.get_mut("sheet"))
                .ok_or_else(|| {
                    bad_input(
                        "capture reference must be a file path or an object with a sheet path",
                    )
                })?;
            absolute(sheet)?;
        }
    }
    Ok(())
}

pub(super) fn apply(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure body-targets", &["model", "out"])?;
    let output = Path::new(required(args, "out")?);
    super::review::preflight(output)?;
    let file = super::super::single(args, "body-targets")?;
    let (value, annotation_bytes) = super::super::intrinsics::read_document(file)?;
    let annotations: Annotations = serde_json::from_value(value)
        .map_err(|e| bad_input(format!("invalid body target annotations: {e}")))?;
    if annotations.schema != "ftx-body-targets/1"
        || !Path::new(&annotations.source_input).is_absolute()
    {
        return Err(bad_input(
            "expected ftx-body-targets/1 with an absolute source_input path",
        ));
    }
    let (mut input, original_bytes) =
        super::super::intrinsics::read_document(&annotations.source_input)?;
    if sha256_hex(&original_bytes) != annotations.source_input_sha256 {
        return Err(bad_input(
            "source input changed; prepare a new model target review",
        ));
    }
    let mut config = configuration(&input)?;
    if config.model_sha256 != annotations.model_sha256 {
        return Err(bad_input(
            "annotation model identity differs from the source input",
        ));
    }
    let names = observed_names(&input)?;
    if names.iter().any(|n| !annotations.keypoints.contains_key(n)) {
        return Err(bad_input(
            "every observed keypoint name must retain a model target; observations are not removed or renamed",
        ));
    }
    config.keypoints = annotations.keypoints;
    let model = model(required(args, "model")?, &config.model_sha256)?;
    let adapter = Adapter::new(model, config, &annotations.model_sha256)?;
    input["model"]["keypoints"] = json!(adapter.configuration().keypoints);
    rebase_capture(&mut input, Path::new(&annotations.source_input))?;
    let mut bundle = Bundle::new();
    let original = bundle.push("source-input.json".into(), original_bytes)?;
    let annotation = bundle.push("body-targets.json".into(), annotation_bytes)?;
    let replay = bundle.push("input.json".into(), workspace::json_bytes(&input)?)?;
    let result = json!({"schema":"ftx-body-target-materialization/1","status":"TARGETS_APPLIED",
        "body_fitted":false,"source_input":annotations.source_input,
        "input_sha256":annotations.source_input_sha256,"model_sha256":annotations.model_sha256,
        "keypoint_count":adapter.configuration().keypoints.len(),"original_input":original,
        "annotations":annotation,"replay_input":replay,
        "no_claim":"Model target definitions passed native validation. Observation names, values, FIT/CHECK roles and source bindings are preserved. Source cameras, observability, anatomy, physical accuracy and real-wearer fit have not been qualified."});
    bundle.publish_report(output, &result, "targets.json")?;
    Ok((
        result,
        0,
        format!(
            "MODEL TARGETS: APPLIED ({} correspondences)\nReview or fit {}/input.json with the ordinary capture/body-fit commands",
            adapter.configuration().keypoints.len(),
            output.display()
        ),
    ))
}

fn page(data: &Value) -> Result<String, Refusal> {
    let payload = serde_json::to_string(data)
        .map_err(|e| bad_input(e.to_string()))?
        .replace('<', "\\u003c");
    Ok(include_str!("target_review.html")
        .replace("__FTX_TARGET_CSP__", "script-src 'self';")
        .replace(
            "__FTX_TARGET_SCRIPT__",
            "<script src=\"editor.js\" defer></script>",
        )
        .replace("__FTX_TARGET_DATA__", &payload))
}
