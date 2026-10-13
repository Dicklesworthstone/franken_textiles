//! Explicit reuse of an accepted image-fit shape. No tape refit and no reuse of image-frame poses.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use ftx_body::{
    AnnyData, AnnyModel, BodyFrames, BodyParams, Pose, TAPE_FIELDS, Wearer,
    accepted_shape::AcceptedShape, default_model_path, pose_deltas, skin_mesh, tape_readings,
    tape_targets,
};
use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use serde_json::{Map, Value, json};

use crate::{
    Args, bad_input, body_json, fitted_body, known_options, load_model, obj, required, workspace,
};

pub(super) struct PreparedBody {
    pub(super) model: AnnyModel,
    pub(super) params: BodyParams,
    pub(super) description: Map<String, Value>,
    pub(super) source_bytes: Option<Vec<u8>>,
}

fn bytes(path: &Path, limit: u64) -> Result<Vec<u8>, Refusal> {
    let file = File::open(path).map_err(|e| workspace::io_error(path, e))?;
    if !file
        .metadata()
        .map_err(|e| workspace::io_error(path, e))?
        .is_file()
    {
        return Err(bad_input("body-fit inputs must be regular files"));
    }
    let mut out = Vec::new();
    file.take(limit + 1)
        .read_to_end(&mut out)
        .map_err(|e| workspace::io_error(path, e))?;
    if out.len() as u64 > limit {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{} exceeds its {limit}-byte input budget", path.display()),
        ));
    }
    Ok(out)
}

fn absent(path: &Path) -> Result<(), Refusal> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(workspace::io_error(path, e)),
        Ok(_) => Err(bad_input(
            "body output must be a new directory; existing evidence is never overwritten",
        )),
    }
}

fn read_report(path: &str) -> Result<(Value, Vec<u8>), Refusal> {
    let data = bytes(Path::new(path), 16 * 1024 * 1024)?;
    let report: Value =
        serde_json::from_slice(&data).map_err(|e| bad_input(format!("body-fit report: {e}")))?;
    if report["schema"] != "ftx-body-fit/1"
        || report["status"] != "ACCEPTED"
        || !report["solution"].is_object()
    {
        return Err(bad_input(
            "--body-fit requires a saved ACCEPTED ftx-body-fit/1 report, not a CLI envelope",
        ));
    }
    Ok((report, data))
}

/// The ordinary path stays unchanged. Import is opt-in and never falls back to fitting on failure.
pub(super) fn prepare(args: &Args, measurements: Option<&Value>) -> Result<PreparedBody, Refusal> {
    let Some(path) = args.options.get("body-fit") else {
        let model = load_model(args)?;
        let (wearer, fit) = fitted_body(
            &model,
            measurements.ok_or_else(|| bad_input("--measurements is required"))?,
        )?;
        return Ok(PreparedBody {
            description: body_json(&wearer, &fit),
            params: fit.params,
            model,
            source_bytes: None,
        });
    };
    let (report, report_bytes) = read_report(path)?;
    let model_path = args
        .options
        .get("model")
        .map(PathBuf::from)
        .or_else(default_model_path)
        .ok_or_else(|| bad_input("give --model for the accepted body fit"))?;
    let model_bytes = bytes(&model_path, 512 * 1024 * 1024)?;
    let hash = sha256_hex(&model_bytes);
    if report["anny_configuration"]["model_sha256"].as_str() != Some(&hash) {
        return Err(bad_input(
            "body-fit model SHA-256 mismatch; no shape was reused or refitted",
        ));
    }
    // Hash and parse the SAME bytes: replacing the path mid-read cannot change the evaluated model.
    let model = AnnyModel::from_data(AnnyData::from_bytes(&model_bytes, true)?)?;
    if model.data().header.anny_version != "0.6.1" || model.data().header.model != "anny" {
        return Err(bad_input(
            "body-fit reuse requires the pinned Anny 0.6.1 model",
        ));
    }
    let shape = AcceptedShape::read(&report, &hash, &model.data().header.labels.local_changes)?;
    let data = measurements.map(crate::round::measurements).transpose()?;
    if let Some(data) = &data {
        let wearer = &data["wearer"];
        if let Some(months) = wearer.get("age_months").filter(|v| !v.is_null()) {
            let months = months
                .as_f64()
                .filter(|n| n.is_finite())
                .ok_or_else(|| bad_input("wearer age must be finite"))?;
            if (months / 12.0 - shape.years).abs() > 1e-10 {
                return Err(bad_input(
                    "measurement wearer age differs from the accepted body-fit configuration",
                ));
            }
        }
        if let Some(sex) = wearer.get("sex").filter(|v| !v.is_null()) {
            let sex = sex
                .as_str()
                .ok_or_else(|| bad_input("wearer sex must be a string"))?;
            if Wearer::gender_of(Some(sex)) != shape.params.phenotype.gender {
                return Err(bad_input(
                    "measurement wearer sex differs from the accepted body-fit configuration",
                ));
            }
        }
    }
    let rest = model.rest(&shape.params)?;
    let anthropometry = model.anthropometry(&rest);
    if !anthropometry.height.is_finite()
        || anthropometry.height <= 0.0
        || !anthropometry.mass.is_finite()
    {
        return Err(bad_input("accepted body has invalid recomputed dimensions"));
    }
    let faces: Vec<_> = model
        .data()
        .faces
        .iter()
        .map(|f| f.map(|i| i as usize))
        .collect();
    // Recompute at the fitter's measurement pose, independent of the requested visualization pose.
    // Small/non-anatomical test models may lack tape landmarks: report unavailable, never invent zero.
    let tape = (|| {
        let posed = model.pose(
            &rest,
            &pose_deltas(&model, &rest, &Pose::a_pose(35.0), 1.0)?,
        )?;
        let frames = BodyFrames::new(&model, &posed)?;
        tape_readings(&faces, anthropometry.height, &posed, &frames)
            .ok_or_else(|| bad_input("body tape sections are unavailable"))
    })();
    let mut readings = Map::new();
    readings.insert("stature".into(), json!(anthropometry.height * 1000.0));
    let tape_status = match tape {
        Ok(values) if values.iter().all(|v| v.is_finite() && *v > 0.0) => {
            readings.extend(
                TAPE_FIELDS
                    .iter()
                    .zip(values)
                    .map(|(n, v)| (n.to_string(), json!(v))),
            );
            json!({"status":"AVAILABLE","pose":"a_pose_35","provenance":"MODEL_FIT"})
        }
        Ok(_) => json!({"status":"UNAVAILABLE","reason":"non-finite or nonpositive tape readings"}),
        Err(e) => json!({"status":"UNAVAILABLE","reason":e.message}),
    };
    let comparisons: Vec<Value> = data
        .as_ref()
        .into_iter()
        .flat_map(tape_targets)
        .map(|t| {
            let predicted = readings.get(&t.name).and_then(Value::as_f64);
            json!({"name":t.name,"target_mm":t.value_mm,"body_mm":predicted,
            "residual_mm":predicted.map(|v| v-t.value_mm),"used_to_refit":false})
        })
        .collect();
    let p = shape.params.phenotype;
    let local: Map<String, Value> = model
        .data()
        .header
        .labels
        .local_changes
        .iter()
        .zip(&shape.params.local_changes)
        .filter(|(_, v)| **v != 0.0)
        .map(|(n, v)| (n.clone(), json!(v)))
        .collect();
    let description = json!({"model":"anny 0.6.1","model_sha256":hash,
        "age_years":shape.years,"gender":p.gender,"provenance":"MODEL_FIT",
        "phenotype":{"gender":p.gender,"age":p.age,"height":p.height,"weight":p.weight,"muscle":p.muscle,"proportions":p.proportions},
        "local_changes":local,"readings_mm":readings,"mass_kg":anthropometry.mass,"tape_evaluation":tape_status,
        "measurement_comparison":comparisons,"model_fit":[],
        "shape_source":{"kind":"accepted_multi_frame_fit","report_sha256":sha256_hex(&report_bytes),
            "input_sha256":shape.input_sha256,"refitted":false,"image_poses_reused":false},
        "source_fit":report,
        "no_claim":"Saved shape reused without refitting; source residuals and assumptions remain in source_fit. Recomputed dimensions are MODEL_FIT, not new evidence. No covariance or real-wearer fit is certified."});
    eprintln!(
        "body: reusing accepted image-fit shape without tape refitting; source {}",
        sha256_hex(&report_bytes)
    );
    Ok(PreparedBody {
        model,
        params: shape.params,
        description: description.as_object().unwrap().clone(),
        source_bytes: Some(report_bytes),
    })
}

pub(super) fn cmd_body(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "body",
        &["body-fit", "measurements", "out", "pose", "model"],
    )?;
    if args.positional.len() != 1 {
        return Err(bad_input("body takes named options, not positional files"));
    }
    let pose_name = args.options.get("pose").map_or("drape", String::as_str);
    let pose = match pose_name {
        "rest" => Pose::rest(),
        "drape" => Pose::drape(),
        "arms-up" => Pose::arms_up(),
        "seated" => Pose::seated(),
        "squat" => Pose::squat(),
        _ => {
            return Err(bad_input(
                "--pose must be drape, rest, arms-up, seated or squat",
            ));
        }
    };
    let out = PathBuf::from(required(args, "out")?);
    absent(&out)?;
    let measurements = args
        .options
        .get("measurements")
        .map(|p| crate::read_json(p))
        .transpose()?;
    let PreparedBody {
        model,
        params,
        mut description,
        source_bytes,
    } = prepare(args, measurements.as_ref().map(|m| &m.0))?;
    let rest = model.rest(&params)?;
    let posed = model.pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?)?;
    let skin = skin_mesh(&posed.vertices, &model.data().faces)?;
    description.insert("schema".into(), json!("ftx-body/1"));
    description.insert("pose".into(), json!(pose_name));
    if let Some((_, bytes)) = measurements {
        description.insert("measurements_sha256".into(), json!(sha256_hex(&bytes)));
    }
    let doc = Value::Object(description);
    let mesh = obj(&skin.vertices, &skin.triangles, None);
    let report = workspace::json_bytes(&doc)?;
    let evidence = source_bytes.ok_or_else(|| bad_input("missing original body-fit evidence"))?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| workspace::io_error(parent, e))?;
    }
    fs::create_dir(&out).map_err(|e| workspace::io_error(&out, e))?;
    workspace::write_new(&out.join("body.obj"), mesh.as_bytes())?;
    workspace::write_new(&out.join("body_fit.json"), &evidence)?;
    workspace::write_new(&out.join("body.json"), &report)?;
    let text = format!(
        "BODY: reused accepted shape without refitting; pose {pose_name}\n{}\nwrote {}",
        doc["no_claim"].as_str().unwrap(),
        out.display()
    );
    Ok((
        json!({"out":out,"files":["body.obj","body_fit.json","body.json"],"body":doc}),
        0,
        text,
    ))
}
