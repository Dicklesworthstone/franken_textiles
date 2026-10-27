//! Local source/constraint review and accepted fitted geometry; never a second fit or collision skin.

use std::{fmt::Write as _, fs, path::Path};

use ftx_body::{PosedBody, RestBody};
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_export::text::xml_escape;
use ftx_measure::body_fit::{BodyFitReport, FitFrame};
use ftx_measure::silhouette::{self, Axis, SurfaceMesh};
use serde_json::{Value, json};

use super::{Adapter, ResolvedTarget};
use crate::{bad_input, workspace};

const MAX_VERTICES: usize = 100_000;
const MAX_FACES: usize = 200_000;
const MAX_BYTES: usize = 256 * 1024 * 1024;

pub(super) fn preflight(path: &Path) -> Result<(), Refusal> {
    match fs::symlink_metadata(path) {
        Ok(_) => {
            return Err(bad_input(
                "mesh output must be a new directory, not an existing path or link",
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(workspace::io_error(path, e)),
    }
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    if path.file_name().is_none() || !parent.is_dir() {
        return Err(bad_input(
            "mesh output needs an existing parent directory and a new directory name",
        ));
    }
    Ok(())
}

pub(super) struct Bundle {
    pub(super) manifest: Value,
    files: Vec<(String, Vec<u8>)>,
}

impl Bundle {
    pub(super) fn new() -> Self {
        Self {
            manifest: Value::Null,
            files: Vec::new(),
        }
    }

    pub(super) fn push(&mut self, name: String, bytes: Vec<u8>) -> Result<Value, Refusal> {
        let total = self
            .files
            .iter()
            .try_fold(bytes.len(), |n, (_, b)| n.checked_add(b.len()));
        if total.is_none_or(|n| n > MAX_BYTES) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "body mesh review exceeds 256 MiB",
            ));
        }
        let entry = json!({"file":name,"bytes":bytes.len(),"sha256":sha256_hex(&bytes)});
        self.files.push((name, bytes));
        Ok(entry)
    }

    pub(super) fn publish(self, path: &Path, report: &Value) -> Result<(), Refusal> {
        self.publish_report(path, report, "fit.json")
    }

    pub(super) fn publish_report(
        self,
        path: &Path,
        report: &Value,
        report_name: &str,
    ) -> Result<(), Refusal> {
        // Prepare every byte before reserving output; a completion report is always the last write.
        let report_bytes = workspace::json_bytes(report)?;
        if self
            .files
            .iter()
            .try_fold(report_bytes.len(), |n, (_, b)| n.checked_add(b.len()))
            .is_none_or(|n| n > MAX_BYTES)
        {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                "body mesh review exceeds 256 MiB",
            ));
        }
        preflight(path)?;
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        builder
            .create(path)
            .map_err(|e| workspace::io_error(path, e))?;
        for (name, bytes) in self.files {
            workspace::write_new(&path.join(name), &bytes)?;
        }
        workspace::write_new(&path.join(report_name), &report_bytes)
    }
}

pub(super) fn source_image(
    bundle: &mut Bundle,
    capture: &Value,
    frame: &FitFrame,
    i: usize,
) -> Result<Value, Refusal> {
    let record = capture["frames"]
        .as_array()
        .and_then(|frames| {
            frames
                .iter()
                .find(|entry| entry["frame"] == json!(frame.frame))
        })
        .ok_or_else(|| bad_input("capture review has no matching source frame"))?;
    let manifest = capture["selection"]["source_manifest"]
        .as_str()
        .ok_or_else(|| bad_input("capture review has no source manifest"))?;
    let base = Path::new(manifest)
        .parent()
        .ok_or_else(|| bad_input("capture manifest has no parent"))?;
    let source = &record["source"];
    let name = source["image"]
        .as_str()
        .ok_or_else(|| bad_input("capture source image is missing"))?;
    let bytes = super::super::video::read_limited(&base.join(name), 128 * 1024 * 1024)?;
    if json!(sha256_hex(&bytes)) != source["image_sha256"] {
        return Err(bad_input(
            "capture image changed during preparation; no review was published",
        ));
    }
    let extension = if bytes.starts_with(&[0xff, 0xd8]) {
        "jpg"
    } else {
        "png"
    };
    bundle.push(format!("source-{i:04}.{extension}"), bytes)
}

fn observed_silhouette(frame: &FitFrame) -> Result<(String, String), Refusal> {
    let Some(observed) = &frame.silhouette else {
        return Ok((String::new(), String::new()));
    };
    silhouette::require_rectified(&frame.camera)?;
    observed.validate(frame.image_size_px)?;
    let mut svg = String::new();
    let mut text = format!(
        "<div class=\"silhouette-review\"><h3>Observed silhouette samples</h3><p>{}</p>\
         <p>Dashed lines show sampled slices; thick segments show observed foreground. Empty slices \
         assert background. Orange is FIT, blue is CHECK. Read-only constraints; no predicted contour \
         or segmentation/occlusion approval.</p><ul>",
        label(&observed.evidence)
    );
    for line in &observed.lines {
        let (partition, role, color) = match line.partition {
            ftx_measure::triangulate::Partition::Fit => ("fit", "FIT", "#b45309"),
            ftx_measure::triangulate::Partition::HeldOut => ("held_out", "CHECK", "#0369a1"),
        };
        let (axis, end) = match line.axis {
            Axis::Horizontal => ("horizontal", frame.image_size_px[0] - 1),
            Axis::Vertical => ("vertical", frame.image_size_px[1] - 1),
        };
        write!(svg, "<g data-silhouette=\"{partition}\" pointer-events=\"none\"><title>{role}: {axis} @ {} px; sigma {} px</title>",
            line.at_px, line.sigma_px).unwrap();
        for (runs, style) in [
            (
                &[[0.0, f64::from(end)]][..],
                "stroke-width=\"1\" stroke-dasharray=\"5 5\"",
            ),
            (
                line.intervals_px.as_slice(),
                "stroke-width=\"6\" stroke-opacity=\"0.6\"",
            ),
        ] {
            for &[lo, hi] in runs {
                let [x1, y1, x2, y2] = match line.axis {
                    Axis::Horizontal => [lo, line.at_px, hi, line.at_px],
                    Axis::Vertical => [line.at_px, lo, line.at_px, hi],
                };
                write!(svg, "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" stroke=\"{color}\" {style}/>").unwrap();
            }
        }
        svg.push_str("</g>");
        let intervals = if line.intervals_px.is_empty() {
            "empty (background)".to_owned()
        } else {
            serde_json::to_string(&line.intervals_px).map_err(|e| bad_input(e.to_string()))?
        };
        write!(
            text,
            "<li>{role} · {axis} @ {} px · sigma {} px · foreground {intervals}</li>",
            line.at_px, line.sigma_px
        )
        .unwrap();
    }
    text.push_str("</ul></div>");
    Ok((svg, text))
}

fn sheet_review(
    bundle: &mut Bundle,
    capture: &Value,
    frame: &FitFrame,
    i: usize,
    image: &str,
) -> Result<(Value, String), Refusal> {
    #[derive(serde::Deserialize)]
    struct Point {
        sheet_xz_m: ftx_core::Vec2,
        pixel_px: ftx_core::Vec2,
        #[serde(default)]
        held_out: bool,
    }
    let record = capture["frames"]
        .as_array()
        .and_then(|records| records.iter().find(|r| r["frame"] == json!(frame.frame)))
        .ok_or_else(|| bad_input("sheet review has no matching camera evidence"))?;
    let pose = &record["sheet_pose"];
    let points: Vec<Point> = serde_json::from_value(pose["input"]["points"].clone())
        .map_err(|e| bad_input(format!("invalid checked sheet points: {e}")))?;
    let [w, h] = frame.image_size_px;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" role=\"img\">\
         <title>Stationary sheet: observed circles, camera-projected crosses</title>\
         <image href=\"{image}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"/>"
    );
    let mut rows = String::new();
    for (j, point) in points.iter().enumerate() {
        // Display the admitted camera on its original distorted pixel frame, never refit or
        // omit held-out points. The stored residual remains the camera gate's own result.
        let p = point.pixel_px;
        let projected =
            frame
                .camera
                .project(Vec3::new(point.sheet_xz_m.x, 0.0, point.sheet_xz_m.y))?;
        let error = pose["reprojection"][j]["error_px"]
            .as_f64()
            .ok_or_else(|| bad_input("checked sheet residual is missing"))?;
        let role = if point.held_out { "CHECK" } else { "FIT" };
        let (x, y) = (projected.x, projected.y);
        write!(svg, "<g data-sheet-point=\"{j}\" data-partition=\"{role}\"><title>Sheet point {j}: {role}; residual {error} px</title>\
            <line x1=\"{}\" y1=\"{}\" x2=\"{x}\" y2=\"{y}\" stroke=\"#555\"/>\
            <circle class=\"observed\" cx=\"{}\" cy=\"{}\" r=\"5\" fill=\"none\" stroke=\"#9333ea\" stroke-width=\"2\"/>\
            <path class=\"projected\" data-x=\"{x}\" data-y=\"{y}\" d=\"M {} {y} h 8 M {x} {} v 8\" stroke=\"#15803d\" stroke-width=\"2\"/>\
            <text x=\"{}\" y=\"{}\" font-size=\"12\">{j} {role}</text></g>",
            p.x, p.y, p.x, p.y, x - 4.0, y - 4.0, p.x + 7.0, p.y - 7.0).unwrap();
        write!(rows, "<li data-sheet-point=\"{j}\" data-partition=\"{role}\" data-error-px=\"{error}\">Point {j} · {role} · sheet (x, z) [{}, {}] m · \
            observed [{}, {}] px · projected [{x}, {y}] px · residual {error} px</li>",
            point.sheet_xz_m.x, point.sheet_xz_m.y, p.x, p.y).unwrap();
    }
    svg.push_str("</svg>\n");
    let name = format!("sheet-{i:04}.svg");
    let artifact = bundle.push(name.clone(), svg.as_bytes().to_vec())?;
    let text = format!(
        "<details class=\"sheet-review\"><summary>Review stationary sheet and camera ({} points)</summary>\
         <p>Reference: <code>{}</code> · lens: <code>{}</code>. Sheet coordinates are metres in the common \
         stationary plane (x, 0, z), z up. Purple circles are observed pixels; green crosses are \
         projections through the checked camera. FIT and CHECK roles remain as originally declared.</p>\
         <p>Read-only: check every named location against the actual sheet before body annotation. \
         A small residual does not verify stationary placement, flatness, print scale or lens accuracy. \
         Correct source reference points before a new capture review; do not tune CHECK points to a fit.</p>\
         <p>Calibration SHA-256: <code>{}</code><br>Reference input SHA-256: <code>{}</code></p>\
         <a href=\"{name}\">Original sheet SVG</a>{svg}\
         <ol>{rows}</ol></details>",
        points.len(),
        label(pose["reference_id"].as_str().unwrap_or("")),
        label(record["lens_id"].as_str().unwrap_or("")),
        label(record["calibration_sha256"].as_str().unwrap_or("")),
        label(record["reference_input_sha256"].as_str().unwrap_or(""))
    );
    Ok((artifact, text))
}

fn silhouette_editability(frame: &FitFrame, capture: &Value) -> (bool, &'static str) {
    if capture["silhouette_input"]["masks"]
        .as_array()
        .is_some_and(|masks| masks.iter().any(|mask| mask["frame"] == json!(frame.frame)))
    {
        return (
            false,
            "Imported mask samples are read-only. Correct their original source and prepare a new review.",
        );
    }
    if silhouette::require_rectified(&frame.camera).is_err() {
        return (
            false,
            "Silhouette authoring requires a valid camera with zero lens distortion. Rectify the image, keypoints and calibration together; do not just zero coefficients.",
        );
    }
    (
        true,
        "The checked camera permits sampled silhouettes. Confirm that image, keypoints and calibration share the same rectified coordinates.",
    )
}

pub(super) fn prepare_capture(
    request: &super::Request,
    capture: &Value,
    original: &[u8],
    replay: &Value,
) -> Result<Bundle, Refusal> {
    let mut bundle = Bundle {
        manifest: Value::Null,
        files: Vec::new(),
    };
    let original_entry = bundle.push("original-input.json".into(), original.to_vec())?;
    let replay_entry = bundle.push("capture.json".into(), workspace::json_bytes(replay)?)?;
    let editor = bundle.push(
        "review.js".into(),
        include_bytes!("capture_review.js").to_vec(),
    )?;
    let mut page = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self'; script-src 'self'; style-src 'unsafe-inline'\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Review capture before fitting</title>\
         <style>body{font:16px system-ui;max-width:1100px;margin:2em auto;padding:0 1em}svg{width:100%;max-height:75vh;border:1px solid #888}\
         section{margin:2em 0}td,th{padding:.4em;text-align:left;overflow-wrap:anywhere}table{width:100%;table-layout:fixed}p{line-height:1.5}\
         button,input,select,textarea{font:inherit;padding:.3em}input{width:9em}input[type=checkbox]{width:auto}textarea{width:95%;max-width:45em}\
         label{margin-right:1em}fieldset{margin:1em 0}tr[data-changed=true]{background:#fff2cf}\
         button[aria-pressed=true]{outline:3px solid #0369a1}svg[data-editing=true]{cursor:crosshair}[hidden]{display:none!important}</style>\
         <h1>Review capture before fitting</h1>\
         <p>PREPARED means current source selection and sheet cameras passed their checks. No body model was loaded or fitted. \
         Orange points are FIT; blue points are CHECK (held out). Review names, target definitions, visibility and coordinates \
         on each original image. Correct coordinates below, then generate a new review directory from the downloaded input. \
         No points or partitions were inferred. Pixels retain their stored orientation and lens distortion.</p>\
         <p>This does not establish anatomical correspondence, observability, physical accuracy, READY measurements or garment fit. \
         Sampled silhouettes are caller-reviewed constraints. Inline samples can be authored on checked rectified frames; imported masks remain read-only.</p>\
         <p><a href=\"capture.json\">Original input for fit-body (requires original source files)</a> · \
         <a href=\"original-input.json\">Original input bytes</a> · <a href=\"review.json\">Complete preparation evidence</a></p>",
    );
    page.push_str("<div id=\"captureEditor\" hidden><h2>Correct point coordinates</h2>\
        <p>Select a point in a table, then click its image or enter stored-pixel coordinates. Names, targets, \
        uncertainty and FIT/CHECK assignments stay fixed. Changes affect only your downloaded input; the original \
        images, SVGs and preparation evidence on disk stay unchanged. Correct points before fitting, not to tune a CHECK result.</p>\
        <fieldset id=\"newPointFields\" disabled><legend>Place a new observation</legend>\
        <p>Choose an existing model target and declare its role and pixel uncertainty before placement. \
        No anatomy is inferred. The fitter still checks observability and every held-out observation.</p>\
        <label>Frame <select id=\"newFrame\"><option value=\"\">Choose frame</option></select></label>\
        <label>Declared target <select id=\"newTarget\"><option value=\"\">Choose target</option></select></label>\
        <label>Role <select id=\"newPartition\"><option value=\"\">Choose FIT or CHECK</option>\
        <option value=\"fit\">FIT</option><option value=\"held_out\">CHECK</option></select></label>\
        <label>Sigma (px) <input id=\"newSigma\" type=\"number\" step=\"any\" min=\"0.000001\" max=\"1000000\"></label>\
        <p id=\"targetDefinition\"></p><button id=\"preparePoint\" type=\"button\">Prepare new point</button> \
        <button id=\"cancelPoint\" type=\"button\" disabled>Cancel placement</button></fieldset>\
        <p id=\"editStatus\" role=\"status\"></p><form id=\"coordinateForm\" novalidate>\
        <label>X (px) <input id=\"editX\" type=\"number\" step=\"any\" disabled></label>\
        <label>Y (px) <input id=\"editY\" type=\"number\" step=\"any\" disabled></label>\
        <button id=\"applyPoint\" type=\"submit\" disabled>Apply coordinates</button> \
        <button id=\"removePoint\" type=\"button\" disabled>Remove selected observation</button></form>\
        <p>Remove a mistaken correspondence before fitting, preserving all other observations. \
        Missing FIT/CHECK evidence can make the native fitter refuse. Do not remove CHECKs to tune a fit result. \
        Reset restores the original complete point lists.</p>\
        <fieldset id=\"silhouetteFields\" disabled><legend>Author sampled body silhouettes</legend>\
        <p>Use already rectified images and cameras. Declare FIT/CHECK slices before fitting and describe clothing and occluders. \
        These are manually observed foreground intervals, not automatic segmentation or evidence of real-body accuracy. \
        Existing slice locations, roles and sigmas stay fixed while correcting endpoints.</p>\
        <label>Frame <select id=\"silhouetteFrame\"><option value=\"\">Choose frame</option></select></label>\
        <p id=\"silhouetteAvailability\"></p><fieldset id=\"silhouetteInputs\" disabled><legend>Slice and observed foreground</legend>\
        <label>Slice <select id=\"silhouetteLine\"><option value=\"\">New slice</option></select></label>\
        <label>Axis <select id=\"silhouetteAxis\"><option value=\"\">Choose axis</option>\
        <option value=\"horizontal\">Horizontal (fixed y)</option><option value=\"vertical\">Vertical (fixed x)</option></select></label>\
        <label>Fixed coordinate (px) <input id=\"silhouetteAt\" type=\"number\" step=\"any\" min=\"0\"></label>\
        <label>Role <select id=\"silhouettePartition\"><option value=\"\">Choose FIT or CHECK</option>\
        <option value=\"fit\">FIT</option><option value=\"held_out\">CHECK</option></select></label>\
        <label>Sigma (px) <input id=\"silhouetteSigma\" type=\"number\" step=\"any\" min=\"0.000001\" max=\"1000000\"></label>\
        <p><label>Foreground intervals: one increasing start, end pair per line<br>\
        <textarea id=\"silhouetteIntervals\" rows=\"3\" placeholder=\"120.5, 245.5&#10;310.5, 380.5\"></textarea></label><br>\
        <label><input id=\"silhouetteBackground\" type=\"checkbox\">Explicit empty/background slice</label></p>\
        <p><label>Evidence for this frame: contour definition, clothing and occluders<br>\
        <textarea id=\"silhouetteEvidence\" rows=\"3\"></textarea></label></p>\
        <p>Enter intervals directly, or choose <strong>Pick endpoints on image</strong>, then click start and end in increasing order. \
        Clicks supply positions along the declared slice; coordinates are not rounded or clamped. \
        At most 64 unique slices and eight disjoint intervals per slice; a silhouette needs some foreground.</p>\
        <button id=\"prepareSilhouette\" type=\"button\">Pick endpoints on image</button> \
        <button id=\"applySilhouette\" type=\"button\">Apply slice</button> \
        <button id=\"removeSilhouette\" type=\"button\" disabled>Remove selected slice</button></fieldset>\
        <button id=\"cancelSilhouette\" type=\"button\">Cancel pending silhouette edit</button></fieldset>\
        <p><button id=\"resetPoints\" type=\"button\">Reset all corrections</button> · \
        <a id=\"correctedInput\" download=\"capture-edited.json\" hidden>Download corrected input</a></p></div>\
        <noscript><p>Annotation editing needs the supplied local script. The original review and input remain usable.</p></noscript>");
    let mut frames = Vec::new();
    for (i, frame) in request.fit.frames.iter().enumerate() {
        let [w, h] = frame.image_size_px;
        if frame.keypoints.len() > 128 {
            return Err(bad_input(
                "capture review supports at most 128 keypoints per frame",
            ));
        }
        let photograph = source_image(&mut bundle, capture, frame, i)?;
        let (silhouette_svg, mut silhouette_text) = observed_silhouette(frame)?;
        if silhouette_text.is_empty() {
            silhouette_text =
                "<div class=\"silhouette-review\"><p>No authored silhouette samples.</p></div>"
                    .into();
        }
        let (silhouette_editable, silhouette_reason) = silhouette_editability(frame, capture);
        let name = photograph["file"].as_str().unwrap();
        let (sheet_svg, sheet_text) = sheet_review(&mut bundle, capture, frame, i, name)?;
        let mut svg = format!(
            "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" role=\"img\">\
            <title>{} @ {} us: supplied keypoints before fitting</title>\
            <image href=\"{name}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"/>",
            label(&frame.frame.stream),
            frame.frame.timestamp_us
        );
        let mut rows = String::new();
        let mut names = std::collections::BTreeSet::new();
        for (j, point) in frame.keypoints.iter().enumerate() {
            let target =
                request.model.keypoints.get(&point.name).ok_or_else(|| {
                    bad_input("capture review has an unknown model correspondence")
                })?;
            let p = point.pixel_px;
            if !names.insert(&point.name)
                || point.name.trim().is_empty()
                || point.name.len() > 4096
                || !p.is_finite()
                || p.x < 0.0
                || p.y < 0.0
                || p.x > f64::from(w - 1)
                || p.y > f64::from(h - 1)
                || !point.sigma_px.is_finite()
                || !(1e-6..=1e6).contains(&point.sigma_px)
            {
                return Err(bad_input(
                    "capture review needs unique named keypoints, valid pixels and positive sigmas",
                ));
            }
            frame.camera.ray(p)?;
            let (partition, color) = match point.partition {
                ftx_measure::triangulate::Partition::Fit => ("FIT", "#b45309"),
                ftx_measure::triangulate::Partition::HeldOut => ("CHECK", "#0369a1"),
            };
            let target =
                label(&serde_json::to_string(target).map_err(|e| bad_input(e.to_string()))?);
            let point_name = label(&point.name);
            write!(svg, "<g data-point=\"{j}\" data-partition=\"{partition}\"><title>{point_name}: {partition}; sigma {} px; {target}</title>\
                <circle cx=\"{}\" cy=\"{}\" r=\"5\" fill=\"none\" stroke=\"{color}\" stroke-width=\"2\"/>\
                <text x=\"{}\" y=\"{}\" fill=\"{color}\" font-family=\"sans-serif\" font-size=\"12\">{point_name} ({partition})</text></g>",
                point.sigma_px, p.x, p.y, p.x + 7.0, p.y - 7.0).unwrap();
            write!(rows, "<tr><td><button type=\"button\" class=\"edit-point\" data-frame=\"{i}\" data-point=\"{j}\" aria-pressed=\"false\" hidden>Select</button> \
                {point_name}</td><td>{partition}</td><td class=\"point-coordinates\">{}, {}</td><td>{}</td><td>{target}</td></tr>",
                p.x, p.y, point.sigma_px).unwrap();
        }
        svg.push_str(&silhouette_svg);
        svg.push_str("</svg>\n");
        let svg_name = format!("frame-{i:04}.svg");
        let overlay = bundle.push(svg_name.clone(), svg.as_bytes().to_vec())?;
        write!(page, "<section data-silhouette-editable=\"{silhouette_editable}\" data-silhouette-reason=\"{}\"><h2>{} @ {} us</h2><p>{w} × {h} stored pixels; {}° clockwise rotation already applied. \
            <a href=\"{svg_name}\">Original keypoint SVG</a></p>{svg}\
            <table><thead><tr><th>Name</th><th>Partition</th><th>Pixel x, y</th><th>Sigma px</th><th>Model target</th></tr></thead><tbody>{rows}</tbody></table>{silhouette_text}{sheet_text}</section>",
            label(silhouette_reason), label(&frame.frame.stream), frame.frame.timestamp_us, frame.rotation_cw_deg).unwrap();
        let mut record = json!({"frame":frame.frame,"image_size_px":frame.image_size_px,
            "rotation_cw_deg":frame.rotation_cw_deg,"keypoints":frame.keypoints,
            "source_image":photograph,"keypoint_svg":overlay,"sheet_svg":sheet_svg});
        if let Some(silhouette) = &frame.silhouette {
            record["silhouette"] = json!(silhouette);
        }
        frames.push(record);
    }
    // Inert JSON must not terminate its script element through a caller-authored label.
    let payload = serde_json::to_string(replay)
        .map_err(|e| bad_input(e.to_string()))?
        .replace('<', "\\u003c");
    writeln!(
        page,
        "<script type=\"application/json\" id=\"captureInput\">{payload}</script>\
        <script src=\"review.js\" defer></script></html>"
    )
    .unwrap();
    let viewer = bundle.push("index.html".into(), page.into_bytes())?;
    bundle.manifest = json!({"schema":"ftx-capture-review/1","status":"PREPARED",
        "body_model_loaded":false,"body_fitted":false,"input_sha256":sha256_hex(original),
        "model_sha256":request.model.model_sha256,"original_input":original_entry,
        "replay_input":replay_entry,"viewer":viewer,"editor":editor,"frames":frames,"capture_input":capture,
        "no_claim":"Source/camera preparation and supplied keypoint/sampled silhouette visualization only. No anatomy, segmentation, full-mask, observability, physical camera, READY measurement or real-wearer fit approval."});
    Ok(bundle)
}

fn obj(vertices: &[Vec3], faces: &[[u32; 3]]) -> Result<Vec<u8>, Refusal> {
    if vertices.is_empty() || vertices.iter().any(|p| !p.is_finite()) {
        return Err(bad_input("mesh review needs finite posed vertices"));
    }
    let mut text = String::from(
        "# FrankenTextiles accepted MODEL_FIT; metres, common calibrated world frame, +Z up\n\
         # Complete model topology; NOT collision-skin admission or real-wearer fit evidence\n",
    );
    for p in vertices {
        // Round-trip f64 values; no grounding, remeshing, rounding to millimetres or pose reset.
        writeln!(text, "v {} {} {}", p.x, p.y, p.z).unwrap();
    }
    for face in faces {
        if face.iter().any(|&i| i as usize >= vertices.len()) {
            return Err(bad_input("mesh face index outside the posed vertex array"));
        }
        writeln!(
            text,
            "f {} {} {}",
            u64::from(face[0]) + 1,
            u64::from(face[1]) + 1,
            u64::from(face[2]) + 1
        )
        .unwrap();
    }
    Ok(text.into_bytes())
}

// Display-only XML sanitation; the complete original label is retained in JSON.
pub(super) fn label(s: &str) -> String {
    xml_escape(
        &s.chars()
            .map(|c| {
                if (c < ' ' && !matches!(c, '\t' | '\n' | '\r'))
                    || matches!(c, '\u{fffe}' | '\u{ffff}')
                {
                    '\u{fffd}'
                } else {
                    c
                }
            })
            .collect::<String>(),
    )
}

fn contours(mesh: &SurfaceMesh, frame: &FitFrame) -> Result<(String, Value), Refusal> {
    let Some(silhouette) = &frame.silhouette else {
        return Ok((String::new(), Value::Null));
    };
    // Reuse the exact coverage/gating kernel, not a raster or hull approximation. Every supplied
    // slice is rechecked, including held-out evidence, on the full exported posed mesh.
    let comparisons =
        silhouette::compare(mesh, &frame.camera, frame.image_size_px, silhouette, None)?;
    if comparisons.iter().any(|line| !line.passed) {
        return Err(bad_input(
            "full mesh silhouette disagrees with an accepted fit/held-out scanline gate",
        ));
    }
    let mut svg = String::new();
    let mut lines = Vec::new();
    for comparison in comparisons {
        // Match by geometric identity; do not assume source evidence was already sorted.
        let observation = silhouette
            .lines
            .iter()
            .find(|line| line.axis == comparison.axis && line.at_px == comparison.at_px)
            .ok_or_else(|| bad_input("missing source silhouette scanline"))?;
        let partition = match observation.partition {
            ftx_measure::triangulate::Partition::Fit => "fit",
            ftx_measure::triangulate::Partition::HeldOut => "held_out",
        };
        let axis = match observation.axis {
            Axis::Horizontal => "horizontal",
            Axis::Vertical => "vertical",
        };
        write!(svg, "<g data-silhouette=\"{partition}\"><title>silhouette {axis} @ {} px: {partition}, boundary error {} px</title>",
            observation.at_px, comparison.boundary_error_px.unwrap()).unwrap();
        for (runs, style) in [
            (
                &observation.intervals_px,
                "stroke=\"#b45309\" stroke-width=\"6\" stroke-opacity=\"0.45\"",
            ),
            (
                &comparison.predicted_intervals_px,
                "stroke=\"#0369a1\" stroke-width=\"2\" stroke-dasharray=\"6 3\"",
            ),
        ] {
            for &[lo, hi] in runs {
                let [x1, y1, x2, y2] = match observation.axis {
                    Axis::Horizontal => [lo, observation.at_px, hi, observation.at_px],
                    Axis::Vertical => [observation.at_px, lo, observation.at_px, hi],
                };
                write!(
                    svg,
                    "<line x1=\"{x1}\" y1=\"{y1}\" x2=\"{x2}\" y2=\"{y2}\" {style}/>"
                )
                .unwrap();
            }
        }
        svg.push_str("</g>");
        lines.push(json!({"observation":observation,"comparison":comparison}));
    }
    Ok((
        svg,
        json!({"evidence":silhouette.evidence,"lines":lines,
        "no_claim":"Agreement on supplied scanlines only, not full-mask, occlusion, segmentation or real-wearer validation. Original oriented rectified coordinates; no resampling."}),
    ))
}

fn projections(
    adapter: &Adapter,
    rest: &RestBody,
    body: &PosedBody,
    frame: &FitFrame,
    photo: Option<&str>,
) -> Result<(String, Value), Refusal> {
    let [w, h] = frame.image_size_px;
    let title = label(&format!(
        "{} @ {} us",
        frame.frame.stream, frame.frame.timestamp_us
    ));
    let background = photo.map_or_else(String::new, |name| {
        format!(
            "<image href=\"{name}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"/>"
        )
    });
    let basis = if photo.is_some() {
        "hash-checked local source image"
    } else {
        "no photograph loaded"
    };
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" overflow=\"hidden\" role=\"img\">\
         <title>{title}: observed and fitted keypoints; {basis}</title>\
         <rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\
         {background}\
         <g font-family=\"sans-serif\" font-size=\"12\">"
    );
    let mut points = Vec::new();
    for observation in &frame.keypoints {
        let index = adapter
            .layout
            .keypoints
            .iter()
            .position(|n| n == &observation.name)
            .ok_or_else(|| bad_input("unknown accepted mesh correspondence"))?;
        // Surface target indices address the sparse selection, not the full model's vertex array.
        let vertex = |i: usize| body.vertices[adapter.selected_vertices[i]];
        let point = match &adapter.targets[index] {
            ResolvedTarget::Vertex(i) => vertex(*i),
            ResolvedTarget::Surface(ids, weights) => {
                vertex(ids[0]) * weights[0]
                    + vertex(ids[1]) * weights[1]
                    + vertex(ids[2]) * weights[2]
            }
            ResolvedTarget::Head(i) => body.bone_poses[*i].t,
            ResolvedTarget::Tail(i) => body.transforms[*i].apply(rest.bone_tails[*i]),
        };
        let projected = frame.camera.project(point)?;
        let error = (projected - observation.pixel_px).norm();
        if !error.is_finite() || error > 3.0 {
            return Err(bad_input(
                "full mesh keypoint disagrees with the accepted 3-pixel fit gate",
            ));
        }
        let observed = observation.pixel_px;
        let name = label(&observation.name);
        let partition = match observation.partition {
            ftx_measure::triangulate::Partition::Fit => "fit",
            ftx_measure::triangulate::Partition::HeldOut => "held_out",
        };
        write!(
            svg,
            "<g><title>{name}: {partition}, residual {error:.6} px</title>\
             <line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"#666\"/>\
             <circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"none\" stroke=\"#b45309\"/>\
             <path d=\"M {} {} h 8 M {} {} v 8\" stroke=\"#0369a1\"/>\
             <text x=\"{}\" y=\"{}\" fill=\"#111\">{name} ({partition})</text></g>",
            observed.x,
            observed.y,
            projected.x,
            projected.y,
            observed.x,
            observed.y,
            projected.x - 4.0,
            projected.y,
            projected.x,
            projected.y - 4.0,
            observed.x + 6.0,
            observed.y - 6.0
        )
        .unwrap();
        points.push(json!({
            "name":observation.name, "partition":observation.partition,
            "sigma_px":observation.sigma_px, "observed_px":observed,
            "predicted_px":projected, "world_m":point, "residual_px":error
        }));
    }
    let mut result = json!({"schema":"ftx-body-reprojections/1","frame":frame.frame,
        "image_size_px":frame.image_size_px,"rotation_cw_deg":frame.rotation_cw_deg,
        "camera":frame.camera,"keypoints":points,
        "no_claim":"Keypoint residuals on an accepted local estimate, not silhouette agreement or occlusion testing. Stored oriented image coordinates; no additional rotation or undistortion. No photograph was loaded."});
    if frame.silhouette.is_some() {
        let mesh = SurfaceMesh {
            vertices_m: body.vertices.clone(),
            triangles: adapter
                .model
                .data()
                .faces
                .iter()
                .map(|t| t.map(|v| v as usize))
                .collect(),
        };
        let (overlay, silhouette) = contours(&mesh, frame)?;
        svg.push_str(&overlay);
        result["silhouette"] = silhouette;
        result["no_claim"] = json!(
            "Keypoint residuals and supplied silhouette scanlines on an accepted local estimate, not full-mask or occlusion testing. Stored oriented image coordinates; no additional rotation or undistortion. No photograph was loaded."
        );
    }
    svg.push_str("</g></svg>\n");
    Ok((svg, result))
}

pub(super) fn prepare(
    adapter: &Adapter,
    report: &BodyFitReport,
    capture: Option<&Value>,
) -> Result<Bundle, Refusal> {
    let solution = report
        .solution
        .as_ref()
        .filter(|_| report.status == "ACCEPTED")
        .ok_or_else(|| bad_input("only an accepted body fit can produce fitted frame meshes"))?;
    let counts = &adapter.model.data().header.counts;
    if counts.vertices == 0 || counts.faces == 0 {
        return Err(bad_input("mesh review needs a nonempty triangle mesh"));
    }
    if counts.vertices > MAX_VERTICES
        || counts.faces > MAX_FACES
        || !(2..=8).contains(&solution.poses.len())
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "mesh review needs 1..100000 vertices, 1..200000 faces and 2..8 fitted frames",
        ));
    }
    if solution.poses.len() != report.input.frames.len() {
        return Err(bad_input("accepted frame/pose counts disagree"));
    }
    let rest = adapter.model.rest(&adapter.parameters(&solution.shape))?;
    let mut bundle = Bundle {
        manifest: Value::Null,
        files: Vec::new(),
    };
    let mut frames = Vec::new();
    let mut page = [
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; ",
        if capture.is_some() { "img-src 'self'; " } else { "" },
        "style-src 'unsafe-inline'\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Accepted body-fit review</title>\
         <style>body{font:16px system-ui;max-width:1100px;margin:2em auto;padding:0 1em}\
         svg{width:100%;max-height:75vh;border:1px solid #888}section{margin:2em 0}\
         a{margin-right:1em}p{line-height:1.5}</style>\
         <h1>Accepted body-fit review</h1>\
         <p>Orange circles are observed keypoints; blue crosses are projections of the complete fitted mesh. \
         Labels retain fit/held-out partitions. ",
        if capture.is_some() { "Hash-checked local source images are copied into this private review. " }
        else { "No photograph is embedded or loaded. " },
        "\
         This is not full-mask, occlusion or real-wearer fit validation.</p>\
         <p>Mesh coordinates are metres in the common calibrated world frame, +Z up. \
         Full topology is preserved; these are not certified collision skins.</p>\
         <p><a href=\"fit.json\">Complete fit and source evidence</a></p>",
    ].concat();
    if report.input.frames.iter().any(|f| f.silhouette.is_some()) {
        page.push_str("<p>Supplied silhouette slices are shown at their original pixel coordinates: \
            thick orange segments are observed foreground, thin dashed blue segments are full-mesh coverage. \
            Gaps are retained; fit and held-out partitions and boundary errors are in each line's tooltip \
            and coordinate JSON. Only supplied slices are tested, not the entire mask.</p>");
    }
    for (i, (frame, pose)) in report.input.frames.iter().zip(&solution.poses).enumerate() {
        if frame.frame != pose.frame {
            return Err(bad_input(
                "accepted mesh pose does not belong to this decoded frame",
            ));
        }
        let body = adapter.model.pose(&rest, &adapter.deltas(&pose.values))?;
        let entry = bundle.push(
            format!("frame-{i:04}.obj"),
            obj(&body.vertices, &adapter.model.data().faces)?,
        )?;
        let photograph = capture
            .map(|capture| source_image(&mut bundle, capture, frame, i))
            .transpose()?;
        let (svg, projected) = projections(
            adapter,
            &rest,
            &body,
            frame,
            photograph.as_ref().and_then(|entry| entry["file"].as_str()),
        )?;
        let svg_name = format!("frame-{i:04}.svg");
        let json_name = format!("frame-{i:04}.json");
        let svg_entry = bundle.push(svg_name.clone(), svg.as_bytes().to_vec())?;
        let points_entry = bundle.push(json_name.clone(), workspace::json_bytes(&projected)?)?;
        write!(page, "<section><h2>{} @ {} us</h2>\
            <p><a href=\"frame-{i:04}.obj\">Full posed OBJ</a>\
            <a href=\"{svg_name}\">Keypoint SVG</a><a href=\"{json_name}\">Coordinates and residuals</a></p>\
            {svg}</section>", label(&frame.frame.stream), frame.frame.timestamp_us).unwrap();
        let mut record = json!({"frame":frame.frame,"mesh":entry,
            "vertices":body.vertices.len(),"faces":counts.faces,
            "reprojection_svg":svg_entry,"reprojections":points_entry});
        if let Some(photograph) = photograph {
            record["source_image"] = photograph;
        }
        frames.push(record);
    }
    page.push_str("</html>\n");
    let viewer = bundle.push("index.html".to_string(), page.into_bytes())?;
    bundle.manifest = json!({
        "schema":"ftx-body-mesh-review/1",
        "model_sha256":adapter.config.model_sha256,
        "units":"m",
        "coordinates":"common_calibrated_world_z_up",
        "export_pose_evaluations":frames.len(),
        "frames":frames,
        "viewer":viewer,
        "no_claim":"Accepted local model estimates, not measured bodies. Full model topology is preserved, including inner shells; meshes are not certified collision skins. No real-wearer fit, watertightness, self-intersection or uncertainty claim."
    });
    Ok(bundle)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_artifact_write_does_not_publish_the_completion_report() {
        let root = std::env::temp_dir().join(format!(
            "ftx-mesh-write-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bundle = Bundle {
            manifest: Value::Null,
            files: vec![
                ("frame-0000.obj".into(), b"first mesh".to_vec()),
                ("absent/frame-0001.obj".into(), b"second mesh".to_vec()),
            ],
        };
        assert!(
            bundle
                .publish(&root, &json!({"status":"ACCEPTED"}))
                .is_err()
        );
        assert_eq!(
            fs::read(root.join("frame-0000.obj")).unwrap(),
            b"first mesh"
        );
        assert!(!root.join("fit.json").exists());
        assert!(preflight(&root).is_err());
    }

    #[test]
    fn mesh_writer_refuses_invalid_vertices_and_indices() {
        assert!(obj(&[], &[]).is_err());
        assert!(obj(&[Vec3::new(f64::NAN, 0., 0.)], &[]).is_err());
        assert!(obj(&[Vec3::ZERO], &[[0, 1, 2]]).is_err());
        assert_eq!(
            label("<x>\0\u{ffff}&\""),
            "&lt;x&gt;\u{fffd}\u{fffd}&amp;&quot;"
        );
    }

    #[test]
    fn contour_review_preserves_gaps_axes_partitions_and_rechecks_held_out_lines() {
        use ftx_core::{Mat3, Vec2};
        use ftx_measure::{
            camera::{Camera, Distortion, Intrinsics},
            silhouette::{Scanline, Silhouette},
            triangulate::{FrameId, Partition},
        };
        let mesh = SurfaceMesh {
            vertices_m: [
                [10., 20.],
                [30., 20.],
                [30., 80.],
                [10., 80.],
                [50., 20.],
                [70., 20.],
                [70., 80.],
                [50., 80.],
            ]
            .map(|[x, y]| Vec3::new(x / 128., y / 128., 1.))
            .to_vec(),
            triangles: vec![[0, 1, 2], [0, 2, 3], [4, 5, 6], [4, 6, 7]],
        };
        let mut frame = FitFrame {
            frame: FrameId {
                stream: "synthetic".into(),
                timestamp_us: 0,
            },
            image_size_px: [100, 100],
            rotation_cw_deg: 0,
            keypoints: vec![],
            camera: Camera {
                intrinsics: Intrinsics {
                    focal_px: Vec2::new(128., 128.),
                    principal_px: Vec2::ZERO,
                    distortion: Distortion::default(),
                },
                world_to_camera: Mat3::IDENTITY,
                center_m: Vec3::ZERO,
            },
            silhouette: Some(Silhouette {
                evidence: "explicit <source> & definitions".into(),
                lines: vec![
                    Scanline {
                        axis: Axis::Vertical,
                        at_px: 20.,
                        intervals_px: vec![[20., 80.]],
                        sigma_px: 2.,
                        partition: Partition::Fit,
                    },
                    Scanline {
                        axis: Axis::Horizontal,
                        at_px: 40.,
                        intervals_px: vec![[10., 30.], [50., 70.]],
                        sigma_px: 1.,
                        partition: Partition::HeldOut,
                    },
                    Scanline {
                        axis: Axis::Horizontal,
                        at_px: 10.,
                        intervals_px: vec![],
                        sigma_px: 1.,
                        partition: Partition::Fit,
                    },
                ],
            }),
        };
        let original = serde_json::to_value(&frame).unwrap();
        assert!(silhouette_editability(&frame, &json!({})).0);
        let imported = json!({"silhouette_input":{"masks":[{"frame":frame.frame}]}});
        assert!(!silhouette_editability(&frame, &imported).0);
        let unrelated = json!({"silhouette_input":{"masks":[{
            "frame":{"stream":"other","timestamp_us":0}
        }]}});
        assert!(silhouette_editability(&frame, &unrelated).0);
        let (observed, text) = observed_silhouette(&frame).unwrap();
        assert_eq!(observed.matches("data-silhouette=").count(), 3);
        assert!(observed.contains("x1=\"20\" y1=\"0\" x2=\"20\" y2=\"99\""));
        assert!(observed.contains("x1=\"50\" y1=\"40\" x2=\"70\" y2=\"40\""));
        assert!(text.contains("empty (background)"));
        assert!(text.contains("sigma 2 px"));
        assert!(!text.contains("<source>"));
        frame.camera.intrinsics.distortion.radial[0] = 0.01;
        assert!(!silhouette_editability(&frame, &json!({})).0);
        assert!(observed_silhouette(&frame).is_err());
        frame.camera.intrinsics.distortion.radial[0] = 0.;
        frame.camera.intrinsics.distortion.tangential[0] = 0.001;
        assert!(!silhouette_editability(&frame, &json!({})).0);
        frame.camera.intrinsics.distortion.tangential[0] = 0.;
        let (svg, data) = contours(&mesh, &frame).unwrap();
        assert!(svg.contains("x1=\"10\" y1=\"40\" x2=\"30\" y2=\"40\""));
        assert!(svg.contains("x1=\"50\" y1=\"40\" x2=\"70\" y2=\"40\""));
        assert!(svg.contains("x1=\"20\" y1=\"20\" x2=\"20\" y2=\"80\""));
        assert!(svg.contains("data-silhouette=\"held_out\""));
        assert!(!svg.contains("<source>"));
        assert_eq!(data["evidence"], original["silhouette"]["evidence"]);
        let lines = data["lines"].as_array().unwrap();
        assert_eq!(lines.len(), 3);
        for (entry, source) in lines.iter().zip([2, 1, 0]) {
            assert_eq!(
                entry["observation"],
                original["silhouette"]["lines"][source]
            );
            assert_eq!(
                entry["comparison"]["predicted_intervals_px"],
                entry["observation"]["intervals_px"]
            );
            assert_eq!(entry["comparison"]["boundary_error_px"], 0.);
            assert_eq!(entry["comparison"]["passed"], true);
        }
        assert_eq!(serde_json::to_value(&frame).unwrap(), original);
        // A previously accepted report cannot authorize changed held-out coverage on export.
        frame.silhouette.as_mut().unwrap().lines[1].intervals_px[1][1] += 10.;
        assert!(contours(&mesh, &frame).is_err());
        frame.silhouette = None;
        let (empty, absent) = contours(&mesh, &frame).unwrap();
        assert!(empty.is_empty());
        assert!(absent.is_null());
    }
}
