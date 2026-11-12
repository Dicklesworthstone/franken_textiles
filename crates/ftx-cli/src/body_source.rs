//! Explicit reuse of an accepted image-fit shape. No tape refit and no reuse of image-frame poses.

use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use ftx_body::{
    AnnyData, AnnyModel, BodyParams, Pose, accepted_shape, default_model_path, pose_deltas,
    skin_mesh,
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

/// Read the exact accepted-report and model bytes once for shape consumers that do not refit.
pub(super) fn accepted_inputs(args: &Args) -> Result<(AnnyModel, String, Vec<u8>), Refusal> {
    let (report, report_bytes) = read_report(required(args, "body-fit")?)?;
    let (model, hash) = identified_model(args)?;
    if report["anny_configuration"]["model_sha256"].as_str() != Some(&hash) {
        return Err(bad_input(
            "body-fit model SHA-256 mismatch; no shape was reused or refitted",
        ));
    }
    Ok((model, hash, report_bytes))
}

fn identified_model(args: &Args) -> Result<(AnnyModel, String), Refusal> {
    identified_model_bound(args, None)
}

fn identified_model_bound(
    args: &Args,
    expected_model_sha: Option<&str>,
) -> Result<(AnnyModel, String), Refusal> {
    let model_path = args
        .options
        .get("model")
        .map(PathBuf::from)
        .or_else(default_model_path)
        .ok_or_else(|| bad_input("give --model to select the fitted body model"))?;
    let model_bytes = bytes(&model_path, 512 * 1024 * 1024)?;
    let hash = sha256_hex(&model_bytes);
    if expected_model_sha.is_some_and(|expected| expected != hash) {
        return Err(bad_input(
            "selected body model changed after flat validation; choose the intended model and run a new round",
        ));
    }
    // Hash and parse the SAME bytes, including the model's array-hash checks.
    let model = AnnyModel::from_data(AnnyData::from_bytes(&model_bytes, true)?)?;
    Ok((model, hash))
}

/// Explicit selected-body inputs for drafting and flat validation. An absent report selects the
/// ordinary tape fitter; a supplied report must pass admission and is never replaced by tape fitting.
pub(super) fn selected_inputs(
    args: &Args,
) -> Result<(AnnyModel, String, Option<Vec<u8>>), Refusal> {
    if args.options.contains_key("body-fit") {
        accepted_inputs(args).map(|(model, hash, source)| (model, hash, Some(source)))
    } else {
        identified_model(args).map(|(model, hash)| (model, hash, None))
    }
}

/// The ordinary path stays unchanged. Import is opt-in and never falls back to fitting on failure.
pub(super) fn prepare(args: &Args, measurements: Option<&Value>) -> Result<PreparedBody, Refusal> {
    prepare_bound(args, measurements, None)
}

/// A round that already checked a selected body must simulate those same model bytes.
pub(super) fn prepare_bound(
    args: &Args,
    measurements: Option<&Value>,
    expected_model_sha: Option<&str>,
) -> Result<PreparedBody, Refusal> {
    let Some(_) = args.options.get("body-fit") else {
        let model = if expected_model_sha.is_some() {
            identified_model_bound(args, expected_model_sha)?.0
        } else {
            load_model(args)?
        };
        let measurements = measurements.ok_or_else(|| bad_input("--measurements is required"))?;
        let (wearer, fit) = fitted_body(&model, measurements)?;
        let mut description = body_json(&wearer, &fit);
        let params =
            if let Some(occupied) = ftx_body::nappy_occupancy(&model, &fit.params, measurements)? {
                description
                    .entry("readings_mm")
                    .or_insert_with(|| json!({}))["seat_over_nappy"] =
                    json!(occupied.seat_over_nappy_mm);
                description.insert("nappy_occupancy".into(), occupied.description);
                occupied.params
            } else {
                fit.params
            };
        return Ok(PreparedBody {
            description,
            params,
            model,
            source_bytes: None,
        });
    };
    let (report, report_bytes) = read_report(required(args, "body-fit")?)?;
    let (model, hash) = identified_model_bound(args, expected_model_sha)?;
    if report["anny_configuration"]["model_sha256"].as_str() != Some(&hash) {
        return Err(bad_input(
            "body-fit model SHA-256 mismatch; no shape was reused or refitted",
        ));
    }
    let data = measurements.map(crate::round::measurements).transpose()?;
    let (shape, mut description) =
        accepted_shape::prepare(&model, &hash, &report_bytes, data.as_ref())?;
    let occupancy = data
        .as_ref()
        .map(|data| ftx_body::nappy_occupancy(&model, &shape.params, data))
        .transpose()?
        .flatten();
    let params = if let Some(occupied) = occupancy {
        description
            .entry("readings_mm")
            .or_insert_with(|| json!({}))["seat_over_nappy"] = json!(occupied.seat_over_nappy_mm);
        description.insert("nappy_occupancy".into(), occupied.description);
        occupied.params
    } else {
        shape.params
    };
    eprintln!(
        "body: reusing accepted image-fit shape without tape refitting; source {}",
        sha256_hex(&report_bytes)
    );
    Ok(PreparedBody {
        model,
        params,
        description,
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
    let pose_geometry =
        ftx_body::body_pose_geometry_json(&model, &rest.vertices, &posed.vertices, &skin)?;
    description.insert("schema".into(), json!("ftx-body/1"));
    description.insert("pose".into(), json!(pose_name));
    description.insert("pose_geometry".into(), pose_geometry);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bound_model_refuses_replaced_bytes_before_parsing_or_fitting() {
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ftx-replaced-model-{}-{nonce}.ftxbody",
            std::process::id()
        ));
        let original = b"original model snapshot";
        fs::write(&path, original).unwrap();
        let expected = sha256_hex(original);
        let replacement = b"different model bytes, deliberately not a valid container";
        fs::write(&path, replacement).unwrap();
        let args = crate::parse(&[
            "drape".into(),
            "--model".into(),
            path.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let refusal = match prepare_bound(&args, Some(&json!({})), Some(&expected)) {
            Ok(_) => panic!("changed model bytes must not reach a tape fit"),
            Err(refusal) => refusal,
        };
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(
            refusal
                .message
                .contains("model changed after flat validation")
        );
        assert_eq!(fs::read(&path).unwrap(), replacement);
        let parse_error = identified_model_bound(&args, Some(&sha256_hex(replacement)))
            .err()
            .expect("matching the byte hash must still run ordinary model parsing");
        assert!(
            !parse_error
                .message
                .contains("changed after flat validation")
        );
    }
}
