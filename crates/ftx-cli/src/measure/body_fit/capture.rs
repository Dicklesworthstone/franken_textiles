//! Join reviewed decoded sources and same-lens sheet cameras; never infer anatomy or stillness.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::body_fit::{BodyFitInput, FitFrame, FitStart, Keypoint, MetricObservation};
use ftx_measure::camera::Camera;
use ftx_measure::photo::intrinsics::checked_calibration;
use ftx_measure::silhouette::Silhouette;
use ftx_measure::triangulate::FrameId;
use ftx_measure::video::FrameMetadata;

use serde::Deserialize;
use serde_json::{Value, json};

use super::{Configuration, Request};
use crate::bad_input;

pub(super) fn run(args: &crate::Args) -> Result<(Value, i32, String), Refusal> {
    crate::known_options(args, "measure capture-review", &["out", "silhouettes"])?;
    let output = Path::new(crate::required(args, "out")?);
    super::review::preflight(output)?;
    let file = super::super::single(args, "capture-review")?;
    let (original, bytes) = super::super::intrinsics::read_document(file)?;
    let mut prepared = original.clone();
    if let Some(path) = super::option(args, "silhouettes") {
        let object = prepared
            .as_object_mut()
            .ok_or_else(|| bad_input("capture input must be an object"))?;
        if object.contains_key("silhouettes") {
            return Err(bad_input("cannot replace a capture silhouette manifest"));
        }
        let path = std::fs::canonicalize(path)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
        object.insert(
            "silhouettes".into(),
            json!(
                path.to_str()
                    .ok_or_else(|| bad_input("mask manifest path must be UTF-8"))?
            ),
        );
    }
    let mut replay = prepared.clone();
    let (request, mut receipt) = prepare_input(Path::new(file), prepared, true)?;
    // The option augments the replay, not the original document named by this receipt.
    receipt["input"] = original;
    let source = receipt["source"]
        .as_str()
        .ok_or_else(|| bad_input("capture source path must be UTF-8"))?;
    let base = Path::new(source)
        .parent()
        .ok_or_else(|| bad_input("capture needs a parent directory"))?;
    // Rebase paths and bind prepared sources. Every source/observation and model target stays exact.
    let absolute = |value: &Value| -> Result<Value, Refusal> {
        let path = local(
            base,
            value
                .as_str()
                .ok_or_else(|| bad_input("missing capture path"))?,
        )?;
        Ok(json!(
            path.to_str()
                .ok_or_else(|| bad_input("capture path must be UTF-8"))?
        ))
    };
    replay["video_manifest"] = absolute(&replay["video_manifest"])?;
    replay["reviewed_sources_sha256"] = receipt["sources_sha256"].clone();
    if replay.get("silhouettes").is_some_and(|v| !v.is_null()) {
        replay["silhouettes"] = absolute(&replay["silhouettes"])?;
    }
    for (i, frame) in replay["fit"]["frames"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .enumerate()
    {
        let object = frame
            .as_object_mut()
            .ok_or_else(|| bad_input("capture frames must be objects"))?;
        object.remove("frame");
        object.insert("source".into(), receipt["frames"][i]["source"].clone());
        object.insert(
            "calibration_sha256".into(),
            receipt["frames"][i]["calibration_sha256"].clone(),
        );
        for key in ["calibration", "reference"] {
            let path = absolute(&object[key])?;
            object.insert(key.into(), path);
        }
    }
    let bundle = super::review::prepare_capture(&request, &receipt, &bytes, &replay)?;
    let result = bundle.manifest.clone();
    bundle.publish_report(output, &result, "review.json")?;
    Ok((
        result,
        0,
        format!(
            "CAPTURE REVIEW: PREPARED ({} frames; no body model loaded or fitted)\nOpen {}/index.html; review the points before fitting {}/capture.json",
            request.fit.frames.len(),
            output.display(),
            output.display()
        ),
    ))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    schema: String,
    video_manifest: String,
    #[serde(default)]
    silhouettes: Option<String>,
    #[serde(default)]
    reviewed_sources_sha256: Option<String>,
    reference_id: String,
    model: Configuration,
    fit: Observations,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Observations {
    schema: String,
    metric_scale_source: String,
    observation_evidence: String,
    frames: Vec<Frame>,
    measurements: Vec<MetricObservation>,
    starts: Vec<FitStart>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    #[serde(default)]
    source: Option<Value>,
    #[serde(default)]
    frame: Option<FrameId>,
    lens_id: String,
    calibration: String,
    #[serde(default)]
    calibration_sha256: Option<String>,
    reference: String,
    keypoints: Vec<Keypoint>,
    #[serde(default)]
    silhouette: Option<Silhouette>,
}

fn local(base: &Path, name: &str) -> Result<PathBuf, Refusal> {
    if name.trim().is_empty() || name.len() > 4096 {
        return Err(bad_input("capture paths need 1..4096 bytes"));
    }
    Ok(base.join(name))
}

fn document(base: &Path, name: &str) -> Result<(Value, Vec<u8>), Refusal> {
    let path = local(base, name)?;
    super::super::intrinsics::read_document(
        path.to_str()
            .ok_or_else(|| bad_input("capture path must be UTF-8"))?,
    )
}

pub(super) fn prepare(path: &Path, original: Value) -> Result<(Request, Value), Refusal> {
    prepare_input(path, original, false)
}

fn prepare_input(path: &Path, original: Value, review: bool) -> Result<(Request, Value), Refusal> {
    let input: Capture = serde_json::from_value(original.clone())
        .map_err(|e| bad_input(format!("invalid reviewed capture: {e}")))?;
    if input.schema != "ftx-anny-capture-input/1"
        || input.fit.schema != "ftx-body-fit-input/1"
        || input.reference_id.trim().is_empty()
        || input.reference_id.len() > 256
        || !(2..=8).contains(&input.fit.frames.len())
    {
        return Err(bad_input(
            "capture needs /1 schemas, a stationary reference_id and 2..8 reviewed frames",
        ));
    }
    let source = std::fs::canonicalize(path)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
    let base = source
        .parent()
        .ok_or_else(|| bad_input("capture needs a parent directory"))?;
    let (selection, _, _) = super::super::video::analyze(&local(base, &input.video_manifest)?)?;
    let selected = selection["selected_sources"]
        .as_array()
        .ok_or_else(|| bad_input("frame selector supplied no source records"))?;
    let mut frames = Vec::new();
    let mut receipts = Vec::new();
    let mut identities = BTreeSet::new();
    let mut epochs = BTreeMap::new();
    for entry in input.fit.frames {
        let declaration = entry.frame.is_some();
        if declaration && (!review || input.reviewed_sources_sha256.is_some()) {
            return Err(bad_input(
                "frame declarations require a new unbound capture-review; fitting needs its complete replay input",
            ));
        }
        let selected_source = match (entry.source, entry.frame) {
            (Some(source), None) => source,
            (None, Some(id)) => selected
                .iter()
                .find(|source| source["metadata"]["frame"] == json!(id))
                .cloned()
                .ok_or_else(|| bad_input("declared capture frame is not currently selected"))?,
            _ => {
                return Err(bad_input(
                    "declare exactly one capture frame identity or complete source",
                ));
            }
        };
        // Match the entire record, including masks and focal/epoch metadata, to the
        // exact current buffers scored by the selector. No second image read or resampling.
        if !selected.contains(&selected_source) {
            return Err(bad_input(
                "capture source changed or is not currently selected; rerun video-select and review the capture",
            ));
        }
        let metadata: FrameMetadata =
            serde_json::from_value(selected_source["metadata"].clone())
                .map_err(|e| bad_input(format!("invalid selected-frame metadata: {e}")))?;
        let id: FrameId = metadata.frame.clone();
        if !identities.insert(id.clone()) {
            return Err(bad_input("duplicate capture frame identity"));
        }
        let size: [u32; 2] = serde_json::from_value(selected_source["image_size_px"].clone())
            .map_err(|e| bad_input(format!("invalid oriented image size: {e}")))?;
        let (calibration, calibration_bytes) = document(base, &entry.calibration)?;
        let calibration_sha256 = sha256_hex(&calibration_bytes);
        if entry
            .calibration_sha256
            .as_ref()
            .is_some_and(|expected| expected != &calibration_sha256)
            || (entry.calibration_sha256.is_none() && !declaration)
        {
            return Err(bad_input(
                "capture calibration bytes changed; review a new capture input",
            ));
        }
        let checked = checked_calibration(&calibration)?;
        if checked.input.lens_id != entry.lens_id
            || checked.input.image_size_px != size
            || checked.input.rotation_cw_deg != metadata.rotation_cw_deg
        {
            return Err(bad_input(
                "capture lens, oriented image size and rotation must match the checked calibration",
            ));
        }
        let epoch = (id.stream.clone(), metadata.camera_epoch.clone());
        let binding = (entry.lens_id.clone(), calibration_sha256.clone());
        if let Some(previous) = epochs.insert(epoch, binding.clone())
            && previous != binding
        {
            return Err(bad_input(
                "one capture stream/lens epoch cannot switch calibration",
            ));
        }
        let intrinsics = checked
            .solution
            .as_ref()
            .ok_or_else(|| bad_input("checked calibration has no solution"))?
            .intrinsics;
        if metadata.focal_px.is_some_and(|f| f != intrinsics.focal_px) {
            return Err(bad_input(
                "selected focal metadata differs from the checked lens; no zoom/crop adjustment is inferred",
            ));
        }
        let (mut reference, reference_bytes) = document(base, &entry.reference)?;
        if !reference.is_object()
            || reference.get("intrinsics").is_some()
            || reference["reference_id"] != input.reference_id
            || reference["image"] != selected_source["image"]
            || reference["image_size_px"] != json!(size)
            || reference["rotation_applied_degrees"] != metadata.rotation_cw_deg
        {
            return Err(bad_input(
                "capture sheet reference must identify this image, stationary reference, size and rotation without inline intrinsics",
            ));
        }
        reference["intrinsics"] =
            serde_json::to_value(intrinsics).map_err(|e| bad_input(e.to_string()))?;
        let pose = Camera::sheet_pose_report(&reference)?;
        if pose["ok"] != true || pose["held_out_points"].as_u64().unwrap_or(0) == 0 {
            return Err(bad_input(
                "capture sheet pose needs passing fit and predeclared held-out points",
            ));
        }
        let camera = serde_json::from_value(pose["camera"].clone())
            .map_err(|e| bad_input(format!("invalid checked sheet camera: {e}")))?;
        frames.push(FitFrame {
            frame: id.clone(),
            camera,
            image_size_px: size,
            rotation_cw_deg: metadata.rotation_cw_deg,
            keypoints: entry.keypoints,
            silhouette: entry.silhouette,
        });
        receipts.push(json!({"frame":id,"source":selected_source,
            "lens_id":entry.lens_id,"calibration_sha256":calibration_sha256,
            "calibration":calibration,"reference_input_sha256":sha256_hex(&reference_bytes),
            "sheet_pose":pose}));
    }
    let mut fit = BodyFitInput {
        schema: input.fit.schema,
        metric_scale_source: input.fit.metric_scale_source,
        observation_evidence: input.fit.observation_evidence,
        frames,
        measurements: input.fit.measurements,
        starts: input.fit.starts,
    };
    let masks = input
        .silhouettes
        .map(|name| {
            let path = local(base, &name)?;
            super::import_masks(
                path.to_str()
                    .ok_or_else(|| bad_input("mask manifest path must be UTF-8"))?,
                &mut fit,
            )
        })
        .transpose()?;
    // No manifest means byte-identical old source evidence. Bind imported mask bytes/samples
    // and decoded buffers when present, never the request's own recursive path/byte identity.
    let mut evidence = json!({"selection":selection,"frames":receipts});
    if let Some(masks) = &masks {
        evidence["silhouette_input"] = masks.clone();
    }
    let sources_sha256 =
        sha256_hex(&serde_json::to_vec(&evidence).map_err(|e| bad_input(e.to_string()))?);
    if input
        .reviewed_sources_sha256
        .as_ref()
        .is_some_and(|expected| expected != &sources_sha256)
    {
        return Err(bad_input(
            "reviewed capture sources changed; prepare and review a new capture",
        ));
    }
    let mut receipt = json!({"schema":"ftx-anny-capture/1","source":source,"input":original,
        "sources_sha256":sources_sha256,
        "selection":selection,"frames":receipts,
        "no_claim":"Source consistency and conditional camera reconstruction only. Caller-reviewed anatomy, stationary reference, decoding/orientation and physical accuracy are not authenticated; sharpness is not stillness and acceptance does not make measurements READY."});
    if let Some(masks) = masks {
        receipt["silhouette_input"] = masks;
    }
    if serde_json::to_vec(&receipt)
        .map_err(|e| bad_input(e.to_string()))?
        .len()
        > 4 * 1024 * 1024
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "capture evidence exceeds 4 MiB; use a shorter reviewed decoded-frame manifest",
        ));
    }
    Ok((
        Request {
            schema: "ftx-anny-body-fit-input/1".into(),
            model: input.model,
            fit,
        },
        receipt,
    ))
}
