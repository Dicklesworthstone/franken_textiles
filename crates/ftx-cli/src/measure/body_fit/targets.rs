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
    let counts = &model.data().header.counts;
    if counts.vertices == 0 || counts.faces == 0 {
        return Err(bad_input(
            "model target review requires a nonempty triangle mesh",
        ));
    }
    if counts.vertices > 100_000 || counts.faces > 200_000 || counts.bones > 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "model target review supports at most 100000 vertices, 200000 triangles and 1024 bones",
        ));
    }
    let adapter = Adapter::for_target_review(model, config, &annotations.model_sha256)?;
    let shape: Vec<_> = adapter
        .config
        .shape_parameters
        .iter()
        .map(|p| p.lower + 0.5 * (p.upper - p.lower))
        .collect();
    let rest = adapter.model.rest(&adapter.parameters(&shape))?;
    if rest.vertices.iter().any(|p| !p.is_finite()) {
        return Err(bad_input("model target review needs finite rest vertices"));
    }
    let preview = json!({"status":"UNFITTED", "pose":"unposed_rest", "units":"m",
        "coordinates":"model_rest_z_up_faces_negative_y",
        "age_years":adapter.config.age_years,"gender":adapter.config.gender,
        "fixed_shape":adapter.config.fixed_shape,
        "shape_values":adapter.config.shape_parameters.iter().zip(&shape).map(|(p,v)|
            json!({"name":p.name,"value":v,"lower":p.lower,"upper":p.upper})).collect::<Vec<_>>(),
        "basis":"Free shape parameters are at the midpoint of their declared bounds. No observation, start, fitted shape, root transform or articulated pose was used."});
    let bones: Vec<_> = adapter
        .model
        .data()
        .header
        .labels
        .bones
        .iter()
        .enumerate()
        .map(|(i, bone)| json!({"name":bone,"head_m":rest.bone_heads[i],"tail_m":rest.bone_tails[i]}))
        .collect();
    let geometry = json!({"model_sha256":annotations.model_sha256,"preview":preview,
        "mesh":{"vertices_m":rest.vertices,"triangles":adapter.model.data().faces},"bones":bones});
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
    input["model"]["keypoints"] = json!(adapter.config.keypoints);
    rebase_capture(&mut input, Path::new(&annotations.source_input))?;
    let mut bundle = Bundle::new();
    let original = bundle.push("source-input.json".into(), original_bytes)?;
    let annotation = bundle.push("body-targets.json".into(), annotation_bytes)?;
    let replay = bundle.push("input.json".into(), workspace::json_bytes(&input)?)?;
    let result = json!({"schema":"ftx-body-target-materialization/1","status":"TARGETS_APPLIED",
        "body_fitted":false,"source_input":annotations.source_input,
        "input_sha256":annotations.source_input_sha256,"model_sha256":annotations.model_sha256,
        "keypoint_count":adapter.config.keypoints.len(),"original_input":original,
        "annotations":annotation,"replay_input":replay,
        "no_claim":"Model target definitions passed native validation. Observation names, values, FIT/CHECK roles and source bindings are preserved. Source cameras, observability, anatomy, physical accuracy and real-wearer fit have not been qualified."});
    bundle.publish_report(output, &result, "targets.json")?;
    Ok((
        result,
        0,
        format!(
            "MODEL TARGETS: APPLIED ({} correspondences)\nReview or fit {}/input.json with the ordinary capture/body-fit commands",
            adapter.config.keypoints.len(),
            output.display()
        ),
    ))
}

fn page(data: &Value) -> Result<String, Refusal> {
    let payload = serde_json::to_string(data)
        .map_err(|e| bad_input(e.to_string()))?
        .replace('<', "\\u003c");
    Ok(format!(
        r##"<!doctype html><html lang="en"><meta charset="utf-8">
<meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self'; style-src 'unsafe-inline'">
<meta name="viewport" content="width=device-width,initial-scale=1"><title>Author body model targets</title>
<style>body{{font:16px system-ui;max-width:1200px;margin:2em auto;padding:0 1em}}p{{line-height:1.5}}canvas{{display:block;width:min(100%,calc(75vh * 9 / 7));height:auto;outline:1px solid #888;touch-action:none}}button,input,select{{font:inherit;padding:.3em}}label{{display:inline-block;margin:.4em}}fieldset{{margin:1em 0}}pre{{white-space:pre-wrap;overflow-wrap:anywhere}}[hidden]{{display:none!important}}.views button{{margin:.2em}}#targetStatus{{min-height:3em}}</style>
<h1>Author body model targets</h1><p><strong>Unfitted model, rest pose.</strong> Free shape parameters are at the midpoint of their declared bounds; fixed shape, age and gender come from your declaration. The view does not use your photographs or a fitted body. Identify anatomy yourself, inspect it from several sides, and choose the exact model location you intend to observe.</p>
<p>Surface picks choose the nearest triangle under the cursor and retain its barycentric weights. Vertex picks choose the closest corner of that visible triangle. Bone endpoints are rig locations and may lie inside the skin. No detector, anatomy or fit is inferred.</p>
<p><a href="source-input.json">Original source input</a> · <a href="model.json">Native preview geometry</a> · <a href="review.json">Review details</a></p>
<details><summary>Exact unfitted preview parameters</summary><pre id="previewDetails"></pre></details>
<div id="targetEditor" hidden><div class="views"><button data-view="front">Front</button><button data-view="back">Back</button><button data-view="left">Wearer's left</button><button data-view="right">Wearer's right</button><button data-view="top">Top</button></div>
<label>Turn <input id="yaw" type="range" min="-180" max="180" step="1" value="0"></label><label>Tilt <input id="pitch" type="range" min="-90" max="90" step="1" value="0"></label><label>Zoom <input id="zoom" type="range" min="0.5" max="6" step="0.1" value="1"></label>
<canvas id="targetCanvas" width="900" height="700" aria-label="Unfitted body surface; click to select a correspondence"></canvas>
<fieldset><legend>Declare a model target</legend><label>Existing or required name <select id="targetList"><option value="">New target</option></select></label><label>Name <input id="targetName" maxlength="256" autocomplete="off"></label><label>Location type <select id="targetKind"><option value="surface">Surface point</option><option value="vertex">Mesh vertex</option><option value="bone_head">Bone head</option><option value="bone_tail">Bone tail</option></select></label><label>Bone <select id="targetBone" disabled><option value="">Choose bone</option></select></label>
<p>Click the visible surface for a vertex or surface point. Select a named rig bone for its head or tail. An existing observation's name cannot be removed or renamed here; its target definition can be corrected explicitly. All image observations and their FIT/CHECK assignments remain unchanged.</p>
<pre id="targetDefinition">Choose a location.</pre><button id="applyTarget" type="button">Apply target</button> <button id="removeTarget" type="button" disabled>Remove target</button> <button id="cancelTarget" type="button">Cancel pending edit</button></fieldset>
<p id="targetStatus" role="status"></p><p><button id="resetTargets" type="button">Reset all target changes</button> · <a id="targetDownload" download="body-targets.json" hidden>Download body-targets.json</a></p>
<p>Apply the download with <code>ftx measure body-targets body-targets.json --model MODEL --out NEW_DIR</code>. This checks the original source bytes and converted model again and writes <code>input.json</code> for ordinary capture review and fitting.</p></div>
<noscript><p>The supplied local script is required to select targets. Original inputs and native preview geometry remain available above.</p></noscript>
<script type="application/json" id="targetData">{payload}</script><script src="editor.js" defer></script></html>
"##
    ))
}
