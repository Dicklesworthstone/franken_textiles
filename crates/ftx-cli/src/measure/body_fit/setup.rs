//! Explicit first-capture choices become the ordinary source-bound input, never an inferred fit.

use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

use ftx_body::{AnnyData, AnnyModel, TAPE_FIELDS};
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_measure::{
    body_fit::{BodyModel, MetricObservation, Parameter, prepare_capture_authoring},
    photo::intrinsics::checked_calibration,
    video::FrameMetadata,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Adapter, Configuration, Joint, capture, review::Bundle};
use crate::{Args, bad_input, known_options, required, workspace};

const ROOTS: [&str; 6] = [
    "root_x_m",
    "root_y_m",
    "root_z_m",
    "root_rx_rad",
    "root_ry_rad",
    "root_rz_rad",
];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Source {
    path: String,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Control {
    name: String,
    lower: f64,
    upper: f64,
    default_value: f64,
    unit: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Bone {
    name: String,
    index: usize,
    parent_index: Option<usize>,
    parent_name: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Catalogue {
    shapes: Vec<Control>,
    roots: Vec<Control>,
    joints: Vec<Bone>,
    measurements: Vec<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Candidate {
    frame_index: usize,
    label: String,
    cohort: String,
    image_size_px: [u32; 2],
    eligible: bool,
    source: Value,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Setup {
    schema: String,
    manifest: Source,
    calibration: Source,
    reference: Source,
    sheet: Source,
    reference_id: String,
    lens_id: String,
    model_sha256: String,
    selection: Value,
    frames: Vec<Candidate>,
    catalogue: Catalogue,
}

#[derive(Deserialize, Serialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
enum Choice {
    Fixed { value: f64 },
    Fit { lower: f64, upper: f64 },
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct NamedChoice {
    name: String,
    choice: Choice,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct JointChoice {
    bone: String,
    axis: Vec3,
    choice: Choice,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Annotations {
    schema: String,
    source_setup: String,
    source_setup_sha256: String,
    frame_indices: Vec<usize>,
    age_years: Option<f64>,
    gender: Option<f64>,
    shapes: Vec<NamedChoice>,
    roots: Vec<NamedChoice>,
    joints: Vec<JointChoice>,
    metric_scale_source: String,
    observation_evidence: String,
    #[serde(default)]
    measurements: Vec<MetricObservation>,
}

struct Loaded {
    setup: Setup,
    model: AnnyModel,
    manifest_bytes: Vec<u8>,
    calibration_bytes: Vec<u8>,
    reference_bytes: Vec<u8>,
    sheet_bytes: Vec<u8>,
    reference: Value,
}

fn path_text(path: &Path) -> Result<String, Refusal> {
    path.to_str()
        .filter(|name| !name.is_empty() && name.len() <= 4096)
        .map(str::to_owned)
        .ok_or_else(|| bad_input("capture setup paths need 1..4096 UTF-8 bytes"))
}

fn document(path: &Path) -> Result<(Source, Value, Vec<u8>), Refusal> {
    let path = std::fs::canonicalize(path).map_err(|e| workspace::io_error(path, e))?;
    let path = path_text(&path)?;
    let (value, bytes) = super::super::intrinsics::read_document(&path)?;
    Ok((
        Source {
            path,
            sha256: sha256_hex(&bytes),
        },
        value,
        bytes,
    ))
}

fn catalogue(model: &AnnyModel) -> Catalogue {
    let labels = &model.data().header.labels;
    let mut shapes: Vec<_> = ["height", "weight", "muscle", "proportions"]
        .into_iter()
        .map(|name| Control {
            name: name.into(),
            lower: 0.0,
            upper: 1.0,
            default_value: 0.5,
            unit: "dimensionless".into(),
        })
        .collect();
    shapes.extend(labels.local_changes.iter().map(|name| Control {
        name: name.clone(),
        lower: -1.0,
        upper: 1.0,
        default_value: 0.0,
        unit: "dimensionless".into(),
    }));
    let roots = ROOTS
        .iter()
        .enumerate()
        .map(|(i, name)| {
            let limit = if i < 3 { 1e6 } else { std::f64::consts::PI };
            Control {
                name: (*name).into(),
                lower: -limit,
                upper: limit,
                default_value: 0.0,
                unit: if i < 3 { "m" } else { "rad" }.into(),
            }
        })
        .collect();
    let joints = labels
        .bones
        .iter()
        .enumerate()
        .skip(1)
        .map(|(index, name)| {
            let parent_index = usize::try_from(labels.bone_parents[index]).ok();
            Bone {
                name: name.clone(),
                index,
                parent_name: parent_index.and_then(|i| labels.bones.get(i)).cloned(),
                parent_index,
            }
        })
        .collect();
    Catalogue {
        shapes,
        roots,
        joints,
        measurements: TAPE_FIELDS.iter().map(|name| (*name).into()).collect(),
    }
}

fn load(
    manifest_path: &Path,
    calibration_path: &Path,
    reference_path: &Path,
    reference_id: &str,
    model_path: &Path,
) -> Result<Loaded, Refusal> {
    if reference_id.trim().is_empty() || reference_id.len() > 256 {
        return Err(bad_input("capture setup needs a stationary reference_id"));
    }
    let (manifest, manifest_bytes, manifest_path) =
        super::super::video::read_manifest(manifest_path)?;
    let (selection, _, _) = super::super::video::analyze(&manifest_path)?;
    if serde_json::to_value(&manifest).map_err(|e| bad_input(e.to_string()))? != selection["input"]
        || super::super::video::read_limited(&manifest_path, 16 * 1024 * 1024)? != manifest_bytes
    {
        return Err(bad_input(
            "manifest changed during setup; prepare a new review",
        ));
    }
    let (calibration, calibration_value, calibration_bytes) = document(calibration_path)?;
    let checked = checked_calibration(&calibration_value)?;
    let intrinsics = checked
        .solution
        .as_ref()
        .ok_or_else(|| bad_input("checked calibration has no usable solution"))?
        .intrinsics;
    let (reference, reference_value, reference_bytes) = document(reference_path)?;
    let declaration: capture::CharucoReference = serde_json::from_value(reference_value.clone())
        .map_err(|e| bad_input(format!("invalid stationary ChArUco declaration: {e}")))?;
    if declaration.schema != "ftx-charuco-sheet-reference/1" || declaration.sheet.trim().is_empty()
    {
        return Err(bad_input(
            "capture setup needs an existing ftx-charuco-sheet-reference/1 declaration",
        ));
    }
    let reference_base = Path::new(&reference.path)
        .parent()
        .ok_or_else(|| bad_input("reference declaration needs a parent directory"))?;
    let (sheet, _, sheet_bytes) = document(&reference_base.join(&declaration.sheet))?;
    let bytes = super::super::video::read_limited(model_path, 256 * 1024 * 1024)?;
    let model_sha256 = sha256_hex(&bytes);
    let model = AnnyModel::from_data(AnnyData::from_bytes(&bytes, true)?)?;
    if model.data().header.anny_version != "0.6.1" || model.data().header.counts.bones == 0 {
        return Err(bad_input(
            "capture setup requires the pinned Anny 0.6.1 model",
        ));
    }
    let indices: BTreeMap<_, _> = manifest
        .frames
        .iter()
        .enumerate()
        .map(|(index, frame)| (frame.metadata.frame.clone(), index))
        .collect();
    let selected = selection["selected_sources"]
        .as_array()
        .ok_or_else(|| bad_input("frame selector supplied no source records"))?;
    let mut frames = Vec::with_capacity(selected.len());
    for source in selected {
        let metadata: FrameMetadata = serde_json::from_value(source["metadata"].clone())
            .map_err(|e| bad_input(format!("invalid selected frame: {e}")))?;
        let image_size_px: [u32; 2] = serde_json::from_value(source["image_size_px"].clone())
            .map_err(|e| bad_input(format!("invalid selected image size: {e}")))?;
        let frame_index = *indices
            .get(&metadata.frame)
            .ok_or_else(|| bad_input("selected frame is absent from its source manifest"))?;
        let eligible = checked.input.image_size_px == image_size_px
            && checked.input.rotation_cw_deg == metadata.rotation_cw_deg
            && metadata.focal_px.is_none_or(|f| f == intrinsics.focal_px);
        frames.push(Candidate {
            frame_index,
            label: format!(
                "{} @ {} µs · lens epoch {} · {}° clockwise",
                metadata.frame.stream,
                metadata.frame.timestamp_us,
                metadata.camera_epoch,
                metadata.rotation_cw_deg
            ),
            cohort: serde_json::to_string(&(&metadata.frame.stream, &metadata.camera_epoch))
                .map_err(|e| bad_input(e.to_string()))?,
            image_size_px,
            eligible,
            source: source.clone(),
        });
    }
    let setup = Setup {
        schema: "ftx-capture-setup/1".into(),
        manifest: Source {
            path: path_text(&manifest_path)?,
            sha256: sha256_hex(&manifest_bytes),
        },
        calibration,
        reference,
        sheet,
        reference_id: reference_id.into(),
        lens_id: checked.input.lens_id,
        model_sha256,
        selection,
        frames,
        catalogue: catalogue(&model),
    };
    Ok(Loaded {
        setup,
        model,
        manifest_bytes,
        calibration_bytes,
        reference_bytes,
        sheet_bytes,
        reference: reference_value,
    })
}

pub(super) fn review(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "measure capture-init-review",
        &["model", "calibration", "reference", "reference-id", "out"],
    )?;
    let output = Path::new(required(args, "out")?);
    super::review::preflight(output)?;
    let loaded = load(
        Path::new(super::super::single(args, "capture-init-review")?),
        Path::new(required(args, "calibration")?),
        Path::new(required(args, "reference")?),
        required(args, "reference-id")?,
        Path::new(required(args, "model")?),
    )?;
    let setup = serde_json::to_value(&loaded.setup).map_err(|e| bad_input(e.to_string()))?;
    let setup_bytes = workspace::json_bytes(&setup)?;
    if setup_bytes.len() > 4 * 1024 * 1024 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "capture setup exceeds 4 MiB; prepare an explicitly shorter manifest",
        ));
    }
    let parent = output
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = std::fs::canonicalize(parent).map_err(|e| workspace::io_error(parent, e))?;
    let setup_path = parent
        .join(
            output
                .file_name()
                .ok_or_else(|| bad_input("output needs a directory name"))?,
        )
        .join("setup.json");
    let annotations = Annotations {
        schema: "ftx-capture-choices/1".into(),
        source_setup: path_text(&setup_path)?,
        source_setup_sha256: sha256_hex(&setup_bytes),
        frame_indices: Vec::new(),
        age_years: None,
        gender: None,
        shapes: Vec::new(),
        roots: ROOTS
            .iter()
            .map(|name| NamedChoice {
                name: (*name).into(),
                choice: Choice::Fixed { value: 0.0 },
            })
            .collect(),
        joints: Vec::new(),
        metric_scale_source: format!(
            "Checked calibration {} and stationary reference {}; physical scale and placement remain conditional on the declared board.",
            loaded.setup.lens_id, loaded.setup.reference_id
        ),
        observation_evidence: String::new(),
        measurements: Vec::new(),
    };
    let mut bundle = Bundle::new();
    let context = bundle.push("setup.json".into(), setup_bytes)?;
    let choices = bundle.push(
        "capture-choices.json".into(),
        workspace::json_bytes(&json!(annotations))?,
    )?;
    let originals = json!({
        "manifest":bundle.push("source-manifest.json".into(), loaded.manifest_bytes)?,
        "calibration":bundle.push("calibration.json".into(), loaded.calibration_bytes)?,
        "reference":bundle.push("reference.json".into(), loaded.reference_bytes)?,
        "sheet":bundle.push("sheet.json".into(), loaded.sheet_bytes)?
    });
    let base = Path::new(&loaded.setup.manifest.path)
        .parent()
        .ok_or_else(|| bad_input("setup manifest needs a parent directory"))?;
    let mut views = Vec::with_capacity(loaded.setup.frames.len());
    for candidate in &loaded.setup.frames {
        let image = super::review::source_snapshot(
            &mut bundle,
            base,
            &candidate.source,
            candidate.frame_index,
        )?;
        // Full source records and u64 timestamps stay in setup.json, outside editable browser data.
        views.push(
            json!({"frame_index":candidate.frame_index,"label":candidate.label,
            "cohort":candidate.cohort,"image_size_px":candidate.image_size_px,
            "eligible":candidate.eligible,"image":image}),
        );
    }
    let editor = super::setup_edit::attach(
        &mut bundle,
        &json!({"annotations":annotations,"catalogue":loaded.setup.catalogue,"frames":views}),
    )?;
    let result = json!({"schema":"ftx-capture-setup-review/1","status":"CONFIGURATION_REQUIRED",
        "body_model_loaded":true,"body_fitted":false,"setup":context,"annotations":choices,
        "editor":editor,"frames":views,
        "source_documents":originals,"model_sha256":loaded.setup.model_sha256,
        "selected_frames":loaded.setup.frames.len(),
        "calibration_compatible_frames":loaded.setup.frames.iter().filter(|f| f.eligible).count(),
        "frame_indices":"Indices in the source manifest sorted natively by stream and decoded timestamp; timestamps are labels only in the editor.",
        "no_claim":"Choose two to eight current selected frames, explicit model controls and bounds, and independent evidence. Sheet acquisition is checked only when constructing the chosen capture. No anatomy, pose, observations, fit acceptance or measurement readiness is inferred."});
    bundle.publish_report(output, &result, "review.json")?;
    Ok((
        result,
        0,
        format!(
            "Open {}/index.html, choose frames and model controls, and download capture-choices.json. Run capture-init with the same model and a new output directory.",
            output.display()
        ),
    ))
}

fn configuration(annotations: &Annotations, model_sha256: &str) -> Result<Configuration, Refusal> {
    let age_years = annotations
        .age_years
        .ok_or_else(|| bad_input("choose age_years before constructing a capture"))?;
    let gender = annotations
        .gender
        .ok_or_else(|| bad_input("choose gender before constructing a capture"))?;
    let mut fixed_shape = BTreeMap::new();
    let mut shape_parameters = Vec::new();
    let mut names = BTreeSet::new();
    for row in &annotations.shapes {
        if !names.insert(&row.name) {
            return Err(bad_input("duplicate shape choice"));
        }
        match row.choice {
            Choice::Fixed { value } => {
                fixed_shape.insert(row.name.clone(), value);
            }
            Choice::Fit { lower, upper } => shape_parameters.push(Parameter {
                name: row.name.clone(),
                lower,
                upper,
            }),
        }
    }
    let mut translation = [0.0; 3];
    let mut rotation = [0.0; 3];
    let mut pose_parameters = Vec::new();
    let mut names = BTreeSet::new();
    if annotations.roots.len() != ROOTS.len() {
        return Err(bad_input(
            "declare exactly one fixed or fitted choice for each of the six root components",
        ));
    }
    for row in &annotations.roots {
        let index = ROOTS
            .iter()
            .position(|name| *name == row.name)
            .ok_or_else(|| bad_input("unknown root component"))?;
        if !names.insert(&row.name) {
            return Err(bad_input("duplicate root choice"));
        }
        match row.choice {
            Choice::Fixed { value } => {
                let limit = if index < 3 { 1e6 } else { std::f64::consts::PI };
                if !value.is_finite() || !(-limit..=limit).contains(&value) {
                    return Err(bad_input(
                        "fixed root value is outside its declared model domain",
                    ));
                }
                if index < 3 {
                    translation[index] = value;
                } else {
                    rotation[index - 3] = value;
                }
            }
            Choice::Fit { lower, upper } => pose_parameters.push(Parameter {
                name: row.name.clone(),
                lower,
                upper,
            }),
        }
    }
    let mut joints = BTreeMap::new();
    for row in &annotations.joints {
        let angle_rad = match row.choice {
            Choice::Fixed { value } => value,
            Choice::Fit { lower, upper } => {
                pose_parameters.push(Parameter {
                    name: format!("joint:{}", row.bone),
                    lower,
                    upper,
                });
                0.0
            }
        };
        if joints
            .insert(
                row.bone.clone(),
                Joint {
                    axis: row.axis,
                    angle_rad,
                },
            )
            .is_some()
        {
            return Err(bad_input("duplicate joint choice"));
        }
    }
    Ok(Configuration {
        model_sha256: model_sha256.into(),
        age_years,
        gender,
        fixed_shape,
        shape_parameters,
        root_translation_m: Vec3::new(translation[0], translation[1], translation[2]),
        root_rotation_rad: Vec3::new(rotation[0], rotation[1], rotation[2]),
        joints,
        pose_parameters,
        keypoints: BTreeMap::new(),
        measurements: annotations
            .measurements
            .iter()
            .map(|observation| observation.name.clone())
            .collect(),
    })
}

pub(super) fn apply(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure capture-init", &["model", "out"])?;
    let output = Path::new(required(args, "out")?);
    super::review::preflight(output)?;
    let file = super::super::single(args, "capture-init")?;
    let (value, bytes) = super::super::intrinsics::read_document(file)?;
    let annotations: Annotations = serde_json::from_value(value)
        .map_err(|e| bad_input(format!("invalid capture choices: {e}")))?;
    if annotations.schema != "ftx-capture-choices/1"
        || !Path::new(&annotations.source_setup).is_absolute()
    {
        return Err(bad_input(
            "expected ftx-capture-choices/1 and an absolute source_setup",
        ));
    }
    let (context, context_bytes) =
        super::super::intrinsics::read_document(&annotations.source_setup)?;
    if sha256_hex(&context_bytes) != annotations.source_setup_sha256 {
        return Err(bad_input(
            "source setup changed; prepare a new capture setup review",
        ));
    }
    let saved: Setup = serde_json::from_value(context.clone())
        .map_err(|e| bad_input(format!("invalid source setup: {e}")))?;
    if saved.schema != "ftx-capture-setup/1"
        || [
            &saved.manifest,
            &saved.calibration,
            &saved.reference,
            &saved.sheet,
        ]
        .iter()
        .any(|source| !Path::new(&source.path).is_absolute())
    {
        return Err(bad_input(
            "capture setup needs schema /1 and absolute source paths",
        ));
    }
    let loaded = load(
        Path::new(&saved.manifest.path),
        Path::new(&saved.calibration.path),
        Path::new(&saved.reference.path),
        &saved.reference_id,
        Path::new(required(args, "model")?),
    )?;
    if serde_json::to_value(&loaded.setup).map_err(|e| bad_input(e.to_string()))? != context {
        return Err(bad_input(
            "capture setup sources, selection, calibration, reference or model changed; prepare a new review",
        ));
    }
    if !(2..=8).contains(&annotations.frame_indices.len()) {
        return Err(bad_input("choose two to eight currently selected frames"));
    }
    let mut indices = BTreeSet::new();
    let mut cohort = None;
    let mut frames = Vec::new();
    let mut reference = loaded.reference;
    reference["sheet"] = json!(loaded.setup.sheet.path);
    for index in &annotations.frame_indices {
        if !indices.insert(*index) {
            return Err(bad_input("duplicate chosen frame index"));
        }
        let frame = loaded
            .setup
            .frames
            .iter()
            .find(|frame| frame.frame_index == *index)
            .ok_or_else(|| bad_input("chosen frame is not currently selected"))?;
        if !frame.eligible {
            return Err(bad_input(
                "chosen frame does not match the checked lens dimensions, rotation or focal metadata",
            ));
        }
        let metadata: FrameMetadata = serde_json::from_value(frame.source["metadata"].clone())
            .map_err(|e| bad_input(format!("invalid chosen frame metadata: {e}")))?;
        let key = (metadata.frame.stream, metadata.camera_epoch);
        if cohort.as_ref().is_some_and(|previous| *previous != key) {
            return Err(bad_input(
                "initial capture setup supports one stream and lens epoch; choose one cohort",
            ));
        }
        cohort = Some(key);
        frames.push(json!({"source":frame.source,"lens_id":loaded.setup.lens_id,
            "calibration":loaded.setup.calibration.path,
            "calibration_sha256":loaded.setup.calibration.sha256,
            "reference":reference,"keypoints":[]}));
    }
    let config = configuration(&annotations, &loaded.setup.model_sha256)?;
    let adapter =
        Adapter::for_target_review(loaded.model, config.clone(), &loaded.setup.model_sha256)?;
    let mut input = json!({"schema":"ftx-anny-capture-input/1",
        "video_manifest":loaded.setup.manifest.path,"reference_id":loaded.setup.reference_id,
        "model":config,"fit":{"schema":"ftx-body-fit-input/1",
            "metric_scale_source":annotations.metric_scale_source,
            "observation_evidence":annotations.observation_evidence,
            "frames":frames,"measurements":annotations.measurements,"starts":[]}});
    let base = Path::new(&annotations.source_setup)
        .parent()
        .ok_or_else(|| bad_input("source setup needs a parent directory"))?;
    let (request, receipt) = capture::prepare_generated(base, input.clone())?;
    if receipt["selection"] != loaded.setup.selection
        || receipt["frames"].as_array().is_none_or(|frames| {
            frames.iter().any(|frame| {
                frame["sheet_acquisition"]["sheet_sha256"] != loaded.setup.sheet.sha256
            })
        })
    {
        return Err(bad_input(
            "capture sources changed during construction; prepare a new setup review",
        ));
    }
    prepare_capture_authoring(&request.fit, adapter.layout())?;
    input["reviewed_sources_sha256"] = receipt["sources_sha256"].clone();
    let mut bundle = Bundle::new();
    let input_file = bundle.push("input.json".into(), workspace::json_bytes(&input)?)?;
    let choices_file = bundle.push("choices.json".into(), bytes)?;
    let setup_file = bundle.push("source-setup.json".into(), context_bytes)?;
    let result = json!({"schema":"ftx-capture-init/1","status":"CAPTURE_DRAFT",
        "body_model_loaded":true,"body_fitted":false,"input":input_file,
        "choices":choices_file,"source_setup":setup_file,
        "model_sha256":loaded.setup.model_sha256,"frame_indices":annotations.frame_indices,
        "capture_input":receipt,
        "no_claim":"Source cameras and explicit model choices passed native preparation. Empty targets, photo observations and starts require the existing authoring steps. Unselected phenotype controls retain model defaults, local changes retain zero and unselected joints retain rest pose. No anatomy, observability, fitted body, READY measurements or physical garment fit is established."});
    bundle.publish_report(output, &result, "construction.json")?;
    Ok((
        result,
        0,
        format!(
            "Constructed {}/input.json. Author model targets, photo observations and distinct starts before fitting.",
            output.display()
        ),
    ))
}
