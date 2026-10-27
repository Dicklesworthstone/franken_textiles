//! Inspect declared initializations with the ordinary model and cameras; never optimize or accept.

use std::{fmt::Write as _, path::Path};

use ftx_body::{AnnyData, AnnyModel};
use ftx_core::{Refusal, RefusalCode, Vec2, receipt::sha256_hex};
use ftx_measure::{
    body_fit::{BodyModel, FitFrame, prepare_start_authoring},
    silhouette::{self, Axis, SurfaceMesh},
};
use serde_json::{Value, json};

use super::{Adapter, Request, capture, import_masks, review, start_edit};
use crate::{Args, bad_input, known_options, required, workspace};

fn mesh_bytes(mesh: &SurfaceMesh) -> Result<Vec<u8>, Refusal> {
    if mesh.vertices_m.is_empty()
        || mesh.vertices_m.len() > 100_000
        || mesh.triangles.is_empty()
        || mesh.triangles.len() > 200_000
        || mesh.vertices_m.iter().any(|p| !p.is_finite())
    {
        return Err(bad_input(
            "initialization mesh is invalid or exceeds review limits",
        ));
    }
    let mut text = String::from(
        "# FrankenTextiles declared INITIALIZATION; metres, calibrated world frame, +Z up\n\
         # No optimization, accepted body, collision-skin admission or real-wearer fit claim\n",
    );
    for p in &mesh.vertices_m {
        writeln!(text, "v {} {} {}", p.x, p.y, p.z).unwrap();
    }
    for face in &mesh.triangles {
        if face.iter().any(|&i| i >= mesh.vertices_m.len()) {
            return Err(bad_input("initialization mesh face index is out of range"));
        }
        writeln!(text, "f {} {} {}", face[0] + 1, face[1] + 1, face[2] + 1).unwrap();
    }
    Ok(text.into_bytes())
}

fn interval(svg: &mut String, axis: Axis, at: f64, range: [f64; 2], color: &str) {
    let [a, b] = match axis {
        Axis::Horizontal => [Vec2::new(range[0], at), Vec2::new(range[1], at)],
        Axis::Vertical => [Vec2::new(at, range[0]), Vec2::new(at, range[1])],
    };
    write!(
        svg,
        "<line x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\" stroke=\"{color}\" stroke-width=\"3\"/>",
        a.x, a.y, b.x, b.y
    )
    .unwrap();
}

fn project(
    adapter: &Adapter,
    frame: &FitFrame,
    shape: &[f64],
    pose: &[f64],
    mesh: Option<&SurfaceMesh>,
    photo: Option<&str>,
) -> (String, Value) {
    let [w, h] = frame.image_size_px;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" overflow=\"hidden\" role=\"img\">\
         <title>Declared initialization, observed points and sampled contours</title>\
         <rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>"
    );
    if let Some(photo) = photo {
        write!(
            svg,
            "<image href=\"{photo}\" width=\"{w}\" height=\"{h}\" preserveAspectRatio=\"none\"/>"
        )
        .unwrap();
    }
    let mut diagnostics = Vec::new();
    let mut failed_vertices = 0usize;
    let mut omitted_faces = 0usize;
    if let Some(mesh) = mesh {
        let pixels: Vec<_> = mesh
            .vertices_m
            .iter()
            .map(|&p| frame.camera.project(p))
            .collect();
        failed_vertices = pixels.iter().filter(|p| p.is_err()).count();
        if let Some(error) = pixels.iter().find_map(|p| p.as_ref().err()) {
            diagnostics.push(json!({"stage":"surface_projection","refusal":error,
                "unprojectable_vertices":failed_vertices}));
        }
        // Straight displayed links are valid only for the existing rectified camera domain.
        // Distorted cameras retain exact projected vertices, without invented curved edges.
        if silhouette::require_rectified(&frame.camera).is_ok() {
            svg.push_str(
                "<g fill=\"none\" stroke=\"#475569\" stroke-opacity=\"0.35\" stroke-width=\"0.6\">",
            );
            for face in &mesh.triangles {
                match [&pixels[face[0]], &pixels[face[1]], &pixels[face[2]]] {
                    [Ok(a), Ok(b), Ok(c)] => {
                        write!(
                            svg,
                            "<path d=\"M {} {} L {} {} L {} {} Z\"/>",
                            a.x, a.y, b.x, b.y, c.x, c.y
                        )
                        .unwrap();
                    }
                    _ => omitted_faces += 1,
                }
            }
        } else {
            svg.push_str("<g fill=\"#475569\" fill-opacity=\"0.45\">");
            for pixel in pixels.iter().filter_map(|p| p.as_ref().ok()) {
                write!(
                    svg,
                    "<circle cx=\"{}\" cy=\"{}\" r=\"0.8\"/>",
                    pixel.x, pixel.y
                )
                .unwrap();
            }
        }
        svg.push_str("</g>");
    }
    let targets = adapter.points(shape, pose);
    if let Err(error) = &targets {
        diagnostics.push(json!({"stage":"model_targets","refusal":error}));
    }
    let mut keypoints = Vec::new();
    for observation in &frame.keypoints {
        let observed = observation.pixel_px;
        let name = review::label(&observation.name);
        write!(svg, "<g><title>{name}: {:?}</title><circle cx=\"{}\" cy=\"{}\" r=\"4\" fill=\"none\" stroke=\"#b45309\"/></g>", observation.partition, observed.x, observed.y).unwrap();
        let index = adapter
            .layout
            .keypoints
            .iter()
            .position(|n| n == &observation.name)
            .unwrap();
        let mut point = json!({"name":observation.name,"partition":observation.partition,
            "sigma_px":observation.sigma_px,"observed_px":observed});
        if let Ok(targets) = &targets {
            point["world_m"] = json!(targets[index]);
            match frame.camera.project(targets[index]) {
                Ok(pixel) => {
                    let error = (pixel - observed).norm();
                    point["predicted_px"] = json!(pixel);
                    if error.is_finite() {
                        point["residual_px"] = json!(error);
                    } else {
                        point["projection_refusal"] =
                            json!(bad_input("initial point residual overflow"));
                    }
                    write!(
                        svg,
                        "<path d=\"M {} {} h 8 M {} {} v 8\" stroke=\"#0369a1\"/>",
                        pixel.x - 4.0,
                        pixel.y,
                        pixel.x,
                        pixel.y - 4.0
                    )
                    .unwrap();
                }
                Err(error) => point["projection_refusal"] = json!(error),
            }
        }
        keypoints.push(point);
    }
    let mut contours = Value::Null;
    if let Some(observation) = &frame.silhouette {
        for line in &observation.lines {
            let width = match line.axis {
                Axis::Horizontal => w,
                Axis::Vertical => h,
            };
            interval(
                &mut svg,
                line.axis,
                line.at_px,
                [0.0, f64::from(width - 1)],
                "#d1d5db",
            );
            for &range in &line.intervals_px {
                interval(&mut svg, line.axis, line.at_px, range, "#b45309");
            }
        }
        if let Some(mesh) = mesh {
            match silhouette::compare(mesh, &frame.camera, frame.image_size_px, observation, None) {
                Ok(lines) => {
                    for line in &lines {
                        for &range in &line.predicted_intervals_px {
                            interval(&mut svg, line.axis, line.at_px, range, "#0369a1");
                        }
                    }
                    contours = json!(lines);
                }
                Err(error) => {
                    diagnostics.push(json!({"stage":"silhouette_projection","refusal":error}))
                }
            }
        }
    }
    svg.push_str("</svg>\n");
    (
        svg,
        json!({"frame":frame.frame,"pose_values":pose,"camera":frame.camera,
        "image_size_px":frame.image_size_px,"rotation_cw_deg":frame.rotation_cw_deg,
        "keypoints":keypoints,"observed_silhouette":frame.silhouette,"silhouette_comparisons":contours,
        "unprojectable_vertices":failed_vertices,"omitted_faces":omitted_faces,"diagnostics":diagnostics}),
    )
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "measure fit-start-review",
        &["model", "out", "silhouettes"],
    )?;
    let output = Path::new(required(args, "out")?);
    review::preflight(output)?;
    let file = super::super::single(args, "fit-start-review")?;
    let (value, bytes) = super::super::intrinsics::read_document(file)?;
    let (mut request, capture_input) = if value["schema"] == "ftx-anny-capture-input/1" {
        let (request, receipt) = capture::prepare(Path::new(file), value)?;
        (request, Some(receipt))
    } else {
        (
            serde_json::from_value::<Request>(value)
                .map_err(|e| bad_input(format!("invalid Anny body-fit request: {e}")))?,
            None,
        )
    };
    if request.schema != "ftx-anny-body-fit-input/1" {
        return Err(bad_input("expected ftx-anny-body-fit-input/1"));
    }
    let model_path = Path::new(required(args, "model")?);
    let model_bytes = super::super::video::read_limited(model_path, 256 * 1024 * 1024)?;
    let hash = sha256_hex(&model_bytes);
    if hash != request.model.model_sha256 {
        return Err(bad_input(
            "model SHA-256 mismatch; no start review was prepared",
        ));
    }
    let model = AnnyModel::from_data(AnnyData::from_bytes(&model_bytes, true)?)?;
    if model.data().header.counts.vertices > 100_000 || model.data().faces.len() > 200_000 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "initialization model exceeds review mesh limits",
        ));
    }
    let adapter = Adapter::new(model, request.model, &hash)?;
    let masks = super::option(args, "silhouettes")
        .map(|path| import_masks(path, &mut request.fit))
        .transpose()?;
    let input = prepare_start_authoring(&request.fit, adapter.layout())?;
    let source =
        std::fs::canonicalize(file).map_err(|e| workspace::io_error(Path::new(file), e))?;
    let mut bundle = review::Bundle::new();
    let original = bundle.push("original-input.json".into(), bytes.clone())?;
    let (editor_page, editor) = start_edit::attach(
        &mut bundle,
        &request.fit,
        &adapter,
        &source,
        &bytes,
        capture_input.as_ref(),
        super::option(args, "silhouettes")
            .zip(masks.as_ref())
            .map(|(path, receipt)| (Path::new(path), receipt)),
    )?;
    let images: Vec<_> = input
        .frames
        .iter()
        .enumerate()
        .map(|(i, frame)| {
            capture_input
                .as_ref()
                .map(|capture| review::source_image(&mut bundle, capture, frame, i))
                .transpose()
        })
        .collect::<Result<_, _>>()?;
    let mut page = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src 'self'; script-src 'self'; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">\
         <title>Body fit initializations</title><style>body{font:16px system-ui;max-width:1100px;margin:2rem auto;padding:1rem}svg,img{max-width:100%;border:1px solid #999}pre{white-space:pre-wrap;overflow-wrap:anywhere}section{border-top:2px solid #999;margin:2rem 0}a{color:#0369a1}button,input,select{font:inherit;padding:.3em}label{display:block;margin:.5em 0}fieldset{margin:1em 0}input{max-width:14em}[hidden]{display:none!important}#startStatus{min-height:3em}</style>\
         <h1>Inspect declared body fit initializations</h1>\
         <p>No fitting or acceptance was performed. Orange marks are observations; blue marks are initial projections. Gray wireframe links are shown only for rectified cameras; distorted cameras show projected vertices. Hidden surfaces remain visible. Failed projections and missing contour coverage remain explicit diagnostics.</p>\
         <p>Keep held-out evidence fixed. Inspect starts before fitting; do not tune observations or initializations to held-out residuals. Author explicit values below and generate a new review directory. No anatomy, observability, collision skin, READY measurements or physical fit is established.</p>\
         <p><a href=\"original-input.json\">Exact original input</a> · <a href=\"review.json\">Complete initialization report</a></p>\
         <p>The original input retains its original relative paths; run it from its original location. This review is not a portable capture archive.</p>",
    );
    page.push_str(&editor_page);
    if input.starts.is_empty() {
        page.push_str("<h2>Original source frames</h2><p>No original starts were declared, so no mesh or projection has been evaluated.</p>");
        for (frame, image) in input.frames.iter().zip(&images) {
            write!(
                page,
                "<section><h3>{} @ {} us</h3>",
                review::label(&frame.frame.stream),
                frame.frame.timestamp_us
            )
            .unwrap();
            if let Some(image) = image {
                write!(
                    page,
                    "<img src=\"{}\" alt=\"Original oriented source frame\">",
                    image["file"].as_str().unwrap()
                )
                .unwrap();
            } else {
                page.push_str("<p>This direct input has no bound source photograph.</p>");
            }
            page.push_str("</section>");
        }
    }
    let mut starts = Vec::new();
    for (s, start) in input.starts.iter().enumerate() {
        write!(page, "<h2>Start {}</h2><pre>{}</pre>", s + 1, review::label(&serde_json::to_string_pretty(&json!({"shape_parameters":adapter.layout.shape,"shape_values":start.shape,"pose_parameters":adapter.layout.pose})).unwrap())).unwrap();
        let mut frames = Vec::new();
        for (f, (frame, pose)) in input.frames.iter().zip(&start.poses).enumerate() {
            let mesh = adapter.surface(&start.shape, &pose.values);
            let mesh_entry = mesh.as_ref().ok().map(mesh_bytes).transpose()?;
            let mesh_entry = mesh_entry
                .map(|bytes| bundle.push(format!("start-{s:02}-frame-{f:02}.obj"), bytes))
                .transpose()?;
            let photo = images[f].as_ref().and_then(|i| i["file"].as_str());
            let (svg, mut record) = project(
                &adapter,
                frame,
                &start.shape,
                &pose.values,
                mesh.as_ref().ok(),
                photo,
            );
            if let Err(error) = mesh {
                record["diagnostics"]
                    .as_array_mut()
                    .unwrap()
                    .push(json!({"stage":"model_surface","refusal":error}));
            }
            let svg_entry = bundle.push(
                format!("start-{s:02}-frame-{f:02}.svg"),
                svg.as_bytes().to_vec(),
            )?;
            let points = record["keypoints"].as_array().unwrap();
            let failed_points = points
                .iter()
                .filter(|p| {
                    p.get("predicted_px").is_none() || p.get("projection_refusal").is_some()
                })
                .count();
            let displaced_points = points
                .iter()
                .filter(|p| p["residual_px"].as_f64().is_some_and(|v| v > 3.0))
                .count();
            let failed_lines = record["silhouette_comparisons"]
                .as_array()
                .map_or(0, |lines| {
                    lines.iter().filter(|line| line["passed"] == false).count()
                });
            write!(page, "<section><h3>{} @ {} us</h3><p>Initial diagnostics: {} unprojectable vertices; {} omitted faces; {failed_points} unprojectable points; {displaced_points} points more than 3 px from observations; {failed_lines} discrepant contour lines; {} evaluation refusals. These counts are not a fit verdict.</p>{svg}<p><a href=\"{}\">Projection SVG</a></p>", review::label(&frame.frame.stream), frame.frame.timestamp_us, record["unprojectable_vertices"], record["omitted_faces"], record["diagnostics"].as_array().unwrap().len(), svg_entry["file"].as_str().unwrap()).unwrap();
            if let Some(entry) = &mesh_entry {
                write!(
                    page,
                    "<p><a href=\"{}\">Full initial posed mesh (OBJ)</a></p>",
                    entry["file"].as_str().unwrap()
                )
                .unwrap();
            }
            write!(page, "<details><summary>Pose, points, contours and projection diagnostics</summary><pre>{}</pre></details></section>", review::label(&serde_json::to_string_pretty(&record).unwrap())).unwrap();
            record["mesh"] = json!(mesh_entry);
            record["overlay"] = svg_entry;
            record["source_image"] = json!(images[f]);
            frames.push(record);
        }
        starts.push(json!({"index":s,"shape_values":start.shape,"frames":frames}));
    }
    page.push_str("</html>\n");
    let viewer = bundle.push("index.html".into(), page.into_bytes())?;
    let report = json!({"schema":"ftx-body-start-review/1","status":"INITIALIZATION_REVIEW",
        "body_fitted":false,"input_sha256":sha256_hex(&bytes),"input_source":source,
        "model_sha256":hash,"model":adapter.layout,"anny_configuration":adapter.config,
        "canonical_input":input,"original_input":original,"viewer":viewer,"starts":starts,"start_editor":editor,
        "capture_input":capture_input,"silhouette_input":masks,
        "no_claim":"Declared initializations evaluated without optimization. No accepted body, observability, uniqueness, anatomical correctness, collision-skin admission, READY measurement or real-wearer fit claim."});
    bundle.publish_report(output, &report, "review.json")?;
    Ok((
        report,
        0,
        format!(
            "BODY FIT INITIALIZATIONS: REVIEW PREPARED ({} starts, {} frames; no fitting)\nOpen {}/index.html",
            input.starts.len(),
            input.frames.len(),
            output.display()
        ),
    ))
}
