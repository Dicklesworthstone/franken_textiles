//! Accepted fitted-frame geometry, not a second fit or a collision-ready skin.

use std::{fmt::Write as _, fs, path::Path};

use ftx_body::{PosedBody, RestBody};
use ftx_core::{Refusal, RefusalCode, Vec3, receipt::sha256_hex};
use ftx_export::text::xml_escape;
use ftx_measure::body_fit::{BodyFitReport, FitFrame};
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
    fn push(&mut self, name: String, bytes: Vec<u8>) -> Result<Value, Refusal> {
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
        workspace::write_new(&path.join("fit.json"), &report_bytes)
    }
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
fn label(s: &str) -> String {
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

fn projections(
    adapter: &Adapter,
    rest: &RestBody,
    body: &PosedBody,
    frame: &FitFrame,
) -> Result<(String, Value), Refusal> {
    let [w, h] = frame.image_size_px;
    let title = label(&format!(
        "{} @ {} us",
        frame.frame.stream, frame.frame.timestamp_us
    ));
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {w} {h}\" overflow=\"hidden\" role=\"img\">\
         <title>{title}: observed and fitted keypoints; no photograph loaded</title>\
         <rect width=\"{w}\" height=\"{h}\" fill=\"white\"/>\
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
    svg.push_str("</g></svg>\n");
    Ok((
        svg,
        json!({"schema":"ftx-body-reprojections/1","frame":frame.frame,
        "image_size_px":frame.image_size_px,"rotation_cw_deg":frame.rotation_cw_deg,
        "camera":frame.camera,"keypoints":points,
        "no_claim":"Keypoint residuals on an accepted local estimate, not silhouette agreement or occlusion testing. Stored oriented image coordinates; no additional rotation or undistortion. No photograph was loaded."}),
    ))
}

pub(super) fn prepare(adapter: &Adapter, report: &BodyFitReport) -> Result<Bundle, Refusal> {
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
    let mut page = String::from(
        "<!doctype html><html lang=\"en\"><meta charset=\"utf-8\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; style-src 'unsafe-inline'\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <title>Accepted body-fit review</title>\
         <style>body{font:16px system-ui;max-width:1100px;margin:2em auto;padding:0 1em}\
         svg{width:100%;max-height:75vh;border:1px solid #888}section{margin:2em 0}\
         a{margin-right:1em}p{line-height:1.5}</style>\
         <h1>Accepted body-fit review</h1>\
         <p>Orange circles are observed keypoints; blue crosses are projections of the complete fitted mesh. \
         Labels retain fit/held-out partitions. No photograph is embedded or loaded. \
         This is not silhouette, occlusion or real-wearer fit validation.</p>\
         <p>Mesh coordinates are metres in the common calibrated world frame, +Z up. \
         Full topology is preserved; these are not certified collision skins.</p>\
         <p><a href=\"fit.json\">Complete fit and source evidence</a></p>",
    );
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
        let (svg, projected) = projections(adapter, &rest, &body, frame)?;
        let svg_name = format!("frame-{i:04}.svg");
        let json_name = format!("frame-{i:04}.json");
        let svg_entry = bundle.push(svg_name.clone(), svg.as_bytes().to_vec())?;
        let points_entry = bundle.push(json_name.clone(), workspace::json_bytes(&projected)?)?;
        write!(page, "<section><h2>{} @ {} us</h2>\
            <p><a href=\"frame-{i:04}.obj\">Full posed OBJ</a>\
            <a href=\"{svg_name}\">Keypoint SVG</a><a href=\"{json_name}\">Coordinates and residuals</a></p>\
            {svg}</section>", label(&frame.frame.stream), frame.frame.timestamp_us).unwrap();
        frames.push(json!({"frame":frame.frame,"mesh":entry,
            "vertices":body.vertices.len(),"faces":counts.faces,
            "reprojection_svg":svg_entry,"reprojections":points_entry}));
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
}
