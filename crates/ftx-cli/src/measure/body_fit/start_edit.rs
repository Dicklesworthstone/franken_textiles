//! Explicit bounded start values; retain source identities and validate through the ordinary fitter.

use std::path::Path;

use ftx_body::{AnnyData, AnnyModel};
use ftx_core::{Refusal, receipt::sha256_hex};
use ftx_measure::body_fit::{BodyFitInput, BodyModel, FitStart, FramePose, prepare_input};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Adapter, Request, capture, import_masks, review, targets};
use crate::{Args, bad_input, known_options, required, workspace};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct PoseValues {
    frame_index: usize,
    values: Vec<f64>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StartValues {
    shape: Vec<f64>,
    poses: Vec<PoseValues>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct SilhouetteImport {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Annotations {
    schema: String,
    source_input: String,
    source_input_sha256: String,
    model_sha256: String,
    capture_sources_sha256: Option<String>,
    silhouettes: Option<SilhouetteImport>,
    starts: Vec<StartValues>,
}

fn receipt_hash(receipt: &Value) -> Result<String, Refusal> {
    Ok(sha256_hex(
        &serde_json::to_vec(receipt).map_err(|e| bad_input(e.to_string()))?,
    ))
}

pub(super) fn attach(
    bundle: &mut review::Bundle,
    original_fit: &BodyFitInput,
    adapter: &Adapter,
    source_path: &Path,
    input_bytes: &[u8],
    capture_receipt: Option<&Value>,
    masks: Option<(&Path, &Value)>,
) -> Result<(String, Value), Refusal> {
    let silhouettes = masks
        .map(|(path, receipt)| -> Result<SilhouetteImport, Refusal> {
            let source = std::fs::canonicalize(path).map_err(|e| workspace::io_error(path, e))?;
            Ok(SilhouetteImport {
                path: source
                    .to_str()
                    .ok_or_else(|| bad_input("mask manifest path must be UTF-8"))?
                    .into(),
                sha256: receipt_hash(receipt)?,
            })
        })
        .transpose()?;
    // Only original frame ordinals reach JavaScript. Native u64 timestamps remain native.
    let starts = original_fit
        .starts
        .iter()
        .map(|start| {
            let poses = original_fit
                .frames
                .iter()
                .enumerate()
                .map(|(frame_index, frame)| {
                    let pose = start
                        .poses
                        .iter()
                        .find(|pose| pose.frame == frame.frame)
                        .ok_or_else(|| bad_input("start is missing its source frame pose"))?;
                    Ok(PoseValues {
                        frame_index,
                        values: pose.values.clone(),
                    })
                })
                .collect::<Result<_, Refusal>>()?;
            Ok(StartValues {
                shape: start.shape.clone(),
                poses,
            })
        })
        .collect::<Result<_, Refusal>>()?;
    let annotations = Annotations {
        schema: "ftx-body-starts/1".into(),
        source_input: source_path
            .to_str()
            .ok_or_else(|| bad_input("source input path must be UTF-8"))?
            .into(),
        source_input_sha256: sha256_hex(input_bytes),
        model_sha256: adapter.config.model_sha256.clone(),
        capture_sources_sha256: capture_receipt
            .and_then(|r| r["sources_sha256"].as_str())
            .map(str::to_owned),
        silhouettes,
        starts,
    };
    let frames: Vec<_> = original_fit
        .frames
        .iter()
        .enumerate()
        .map(|(frame_index, frame)| {
            json!({"frame_index":frame_index,"label":format!("Frame {}: {} @ {} us",
                frame_index + 1, frame.frame.stream, frame.frame.timestamp_us)})
        })
        .collect();
    let shape: Vec<_> = adapter
        .layout
        .shape
        .iter()
        .map(|p| json!({"name":p.name,"lower":p.lower,"upper":p.upper,"unit":"dimensionless"}))
        .collect();
    let pose: Vec<_> = adapter
        .layout
        .pose
        .iter()
        .map(|p| {
            let unit = if matches!(p.name.as_str(), "root_x_m" | "root_y_m" | "root_z_m") {
                "m"
            } else {
                "rad"
            };
            json!({"name":p.name,"lower":p.lower,"upper":p.upper,"unit":unit})
        })
        .collect();
    let data = json!({"annotations":annotations,"shape":shape,"pose":pose,"frames":frames});
    let annotation = bundle.push(
        "body-starts.json".into(),
        workspace::json_bytes(&json!(annotations))?,
    )?;
    let editor = bundle.push(
        "start-editor.js".into(),
        include_bytes!("start_edit.js").to_vec(),
    )?;
    Ok((
        page(&data)?,
        json!({"annotations":annotation,"script":editor,"frames":frames,
            "frame_indices":"Original source fit.frames order; timestamps are never round-tripped through JavaScript.",
            "preview":"Original declared starts only; edited values require native materialization and a new review."}),
    ))
}

fn decoded_starts(
    annotations: Vec<StartValues>,
    fit: &BodyFitInput,
) -> Result<Vec<FitStart>, Refusal> {
    if !(2..=4).contains(&annotations.len()) {
        return Err(bad_input(
            "materializing starts requires 2..4 declared initializations",
        ));
    }
    annotations
        .into_iter()
        .map(|start| {
            if start.poses.len() != fit.frames.len() {
                return Err(bad_input(
                    "each start needs exactly one pose per source frame",
                ));
            }
            let mut values = vec![None; fit.frames.len()];
            for pose in start.poses {
                let slot = values
                    .get_mut(pose.frame_index)
                    .ok_or_else(|| bad_input("start pose frame_index is out of range"))?;
                if slot.replace(pose.values).is_some() {
                    return Err(bad_input("duplicate start pose frame_index"));
                }
            }
            let poses = fit
                .frames
                .iter()
                .zip(values)
                .map(|(frame, values)| {
                    Ok(FramePose {
                        frame: frame.frame.clone(),
                        values: values
                            .ok_or_else(|| bad_input("missing start pose frame_index"))?,
                    })
                })
                .collect::<Result<_, Refusal>>()?;
            Ok(FitStart {
                shape: start.shape,
                poses,
            })
        })
        .collect()
}

pub(super) fn apply(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure fit-starts", &["model", "out"])?;
    let output = Path::new(required(args, "out")?);
    review::preflight(output)?;
    let file = super::super::single(args, "fit-starts")?;
    let (value, annotation_bytes) = super::super::intrinsics::read_document(file)?;
    let annotations: Annotations = serde_json::from_value(value)
        .map_err(|e| bad_input(format!("invalid initialization annotations: {e}")))?;
    let source_path = Path::new(&annotations.source_input);
    if annotations.schema != "ftx-body-starts/1" || !source_path.is_absolute() {
        return Err(bad_input(
            "expected ftx-body-starts/1 and an absolute source_input",
        ));
    }
    let (mut input, original_bytes) =
        super::super::intrinsics::read_document(&annotations.source_input)?;
    if sha256_hex(&original_bytes) != annotations.source_input_sha256 {
        return Err(bad_input(
            "source input changed; prepare a new initialization review",
        ));
    }
    let (mut request, capture_input) = if input["schema"] == "ftx-anny-capture-input/1" {
        let (request, receipt) = capture::prepare(source_path, input.clone())?;
        (request, Some(receipt))
    } else {
        (
            serde_json::from_value::<Request>(input.clone())
                .map_err(|e| bad_input(format!("invalid Anny body-fit request: {e}")))?,
            None,
        )
    };
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(bad_input("expected ftx-anny-body-fit-input/1"));
    }
    let capture_hash = capture_input
        .as_ref()
        .and_then(|r| r["sources_sha256"].as_str());
    if annotations.capture_sources_sha256.as_deref() != capture_hash {
        return Err(bad_input(
            "capture sources changed or are not bound; prepare a new initialization review",
        ));
    }
    let model_path = Path::new(required(args, "model")?);
    let model_bytes = super::super::video::read_limited(model_path, 256 * 1024 * 1024)?;
    let hash = sha256_hex(&model_bytes);
    if hash != annotations.model_sha256 || hash != request.model.model_sha256 {
        return Err(bad_input("model SHA-256 mismatch; no starts were applied"));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(&model_bytes, true)?)?;
    let adapter = Adapter::new(model, request.model, &hash)?;
    let mut imported = Vec::new();
    let masks = annotations
        .silhouettes
        .as_ref()
        .map(|binding| {
            if !Path::new(&binding.path).is_absolute() {
                return Err(bad_input("bound mask manifest path must be absolute"));
            }
            let absent: Vec<_> = request
                .fit
                .frames
                .iter()
                .map(|f| f.silhouette.is_none())
                .collect();
            let receipt = import_masks(&binding.path, &mut request.fit)?;
            if receipt_hash(&receipt)? != binding.sha256 {
                return Err(bad_input(
                    "silhouette import changed; prepare a new initialization review",
                ));
            }
            for (index, (frame, was_absent)) in request.fit.frames.iter().zip(absent).enumerate() {
                if was_absent && frame.silhouette.is_some() {
                    imported.push(index);
                    input["fit"]["frames"][index]["silhouette"] = json!(frame.silhouette);
                }
            }
            Ok(receipt)
        })
        .transpose()?;
    request.fit.starts = decoded_starts(annotations.starts, &request.fit)?;
    // One validator: the exact same complete-start contract used by ordinary fitting.
    prepare_input(&request.fit, adapter.layout())?;
    input["fit"]["starts"] = json!(request.fit.starts);
    targets::rebase_capture(&mut input, source_path)?;
    let mut bundle = review::Bundle::new();
    let original = bundle.push("original-input.json".into(), original_bytes)?;
    let annotation = bundle.push("body-starts.json".into(), annotation_bytes)?;
    let replay = bundle.push("input.json".into(), workspace::json_bytes(&input)?)?;
    let report = json!({"schema":"ftx-body-start-materialization/1","status":"STARTS_APPLIED",
        "body_fitted":false,"source_input":annotations.source_input,
        "input_sha256":annotations.source_input_sha256,"model_sha256":hash,
        "capture_sources_sha256":capture_hash,"silhouette_input":masks,
        "materialized_silhouette_frame_indices":imported,
        "start_count":request.fit.starts.len(),"frame_count":request.fit.frames.len(),
        "original_input":original,"annotations":annotation,"replay_input":replay,
        "no_claim":"Explicit initializations passed ordinary native validation. Only starts, rebased local capture paths and separately requested silhouette imports were materialized. No optimization, anatomy, observability, accepted body or real-wearer fit claim."});
    bundle.publish_report(output, &report, "starts.json")?;
    Ok((
        report,
        0,
        format!(
            "BODY FIT INITIALIZATIONS: APPLIED ({} starts, {} frames; no fitting)\nReview {}/input.json with measure fit-start-review, then use ordinary body-fit",
            request.fit.starts.len(),
            request.fit.frames.len(),
            output.display()
        ),
    ))
}

fn page(data: &Value) -> Result<String, Refusal> {
    let payload = serde_json::to_string(data)
        .map_err(|e| bad_input(e.to_string()))?
        .replace('<', "\\u003c");
    Ok(format!(
        r##"
<section><h2>Author initialization values</h2>
<p>Enter shared shape and one pose for each source frame. Values must be strictly inside the named bounds. A complete download needs 2–4 distinct starts. Empty drafts do not guess a body or pose; the midpoint button is an explicit starting guess only.</p>
<p><a href="body-starts.json">Original initialization annotations</a></p>
<p id="startPreviewNotice"><strong>All photographs, projections and meshes below show the original input.</strong> Editing values does not regenerate them. Materialize the download and generate a new native review to inspect edited starts.</p>
<div id="startEditor" hidden>
<label>Applied initialization <select id="startList" aria-label="Applied initialization"></select></label>
<p><button id="newStart" type="button">Add blank start</button> <button id="duplicateStart" type="button">Duplicate selected start</button> <button id="removeStart" type="button">Remove selected start</button></p>
<p><button id="midpointStart" type="button">Use bounds midpoints as a guess</button></p>
<div id="startParameters"></div>
<p><button id="applyStart" type="button">Apply this start</button> <button id="cancelStart" type="button">Cancel pending edit</button></p>
<p id="startStatus" role="status"></p>
<p><button id="resetStarts" type="button">Reset all start changes</button> · <a id="startDownload" download="body-starts.json" hidden>Download body-starts.json</a></p>
<p>Apply with <code>ftx measure fit-starts body-starts.json --model MODEL --out NEW_DIR</code>. Then run <code>ftx measure fit-start-review NEW_DIR/input.json --model MODEL --out NEW_REVIEW</code> before fitting. Keep observations and FIT/CHECK roles fixed; do not tune starts to held-out residuals.</p>
</div><noscript><p>The supplied local script is required to edit starts. Original input and previews remain available.</p></noscript>
<script type="application/json" id="startData">{payload}</script><script src="start-editor.js" defer></script></section>
"##
    ))
}
