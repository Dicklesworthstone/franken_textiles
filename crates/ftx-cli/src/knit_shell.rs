//! Anatomical knit target extraction from explicit source geometry or an accepted Anny body.

use std::path::{Path, PathBuf};

use ftx_body::knit_shell::{
    AcceptedAnatomicalShellRequest, MAX_ANATOMICAL_INPUT_BYTES, anatomical_shell_json,
    build_accepted_anatomical_shell, serialize_anatomical_shell,
};
use ftx_core::receipt::sha256_hex;
use ftx_core::shaping::EaseShellMesh;
use ftx_core::{Refusal, Vec3};
use serde_json::{Value, json};

use crate::{Args, bad_input, body_source, known_options, required, reserve_output, workspace};

fn read(path: &str, label: &str) -> Result<Vec<u8>, Refusal> {
    crate::read_regular_bounded(Path::new(path), MAX_ANATOMICAL_INPUT_BYTES as u64, label)
}

/// Dispatch target for `ftx knit shell`; numerical extraction is shared with portable callers.
pub(crate) fn cmd_knit_shell(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit shell",
        &[
            "input",
            "body-fit",
            "model",
            "region",
            "ease",
            "pose",
            "skinning",
            "thumb-pose",
            "out",
        ],
    )?;
    if args.positional.len() != 2 {
        return Err(bad_input(
            "usage: ftx knit shell --input SOURCE.json --out NEW_DIR, or --body-fit ACCEPTED.json --model FILE --region REGION --ease EASE.json --out NEW_DIR",
        ));
    }
    let out = PathBuf::from(required(args, "out")?);
    match crate::round::symlink_metadata(&out) {
        Ok(_) => {
            return Err(bad_input(
                "knit shell output already exists; use a new directory",
            ));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(workspace::io_error(&out, e)),
    }
    let (shell_json, source_files, source_description) = if let Some(path) =
        args.options.get("input")
    {
        if args
            .options
            .keys()
            .any(|key| !matches!(key.as_str(), "input" | "out"))
        {
            return Err(bad_input(
                "--input contains its complete anatomy and ease; body-model options require the --body-fit route",
            ));
        }
        let bytes = read(path, "anatomical shell source")?;
        let text = std::str::from_utf8(&bytes)
            .map_err(|e| bad_input(format!("anatomical shell source: {e}")))?;
        let shell_json = anatomical_shell_json(text)?;
        let description = json!({"kind":"explicit_anatomy","source_sha256":sha256_hex(&bytes)});
        (shell_json, vec![("source.json", bytes)], description)
    } else {
        required(args, "body-fit")?;
        let ease_bytes = read(required(args, "ease")?, "anatomical ease profile")?;
        let ease = serde_json::from_slice(&ease_bytes)
            .map_err(|e| bad_input(format!("anatomical ease profile: {e}")))?;
        let region = serde_json::from_value(json!(required(args, "region")?))
            .map_err(|e| bad_input(format!("--region: {e}")))?;
        let pose = serde_json::from_value(json!(
            args.options.get("pose").map_or("drape", String::as_str)
        ))
        .map_err(|e| bad_input(format!("--pose: {e}")))?;
        let request = AcceptedAnatomicalShellRequest {
            region,
            ease,
            pose,
            skinning: crate::parse_skinning(args)?,
            thumb_pose: crate::parse_thumb_pose(args)?,
        };
        let (model, model_sha256, report) = body_source::accepted_inputs(args)?;
        let shell = build_accepted_anatomical_shell(&model, &model_sha256, &report, &request)?;
        let source_description = json!({
            "kind":"accepted_anny_body", "model_sha256":model_sha256,
            "body_fit_sha256":sha256_hex(&report), "request":request,
        });
        let request_bytes =
            serde_json::to_vec_pretty(&request).map_err(|e| bad_input(e.to_string()))?;
        let shell_json = serialize_anatomical_shell(&shell)?;
        (
            shell_json,
            vec![("body_fit.json", report), ("request.json", request_bytes)],
            source_description,
        )
    };
    let shell: EaseShellMesh =
        serde_json::from_str(&shell_json).map_err(|e| bad_input(e.to_string()))?;
    let vertices: Vec<_> = shell.vertices_m.iter().copied().map(Vec3::from).collect();
    let triangles: Vec<_> = shell
        .triangles
        .iter()
        .map(|t| t.map(|v| v as usize))
        .collect();
    let obj = crate::obj(&vertices, &triangles, None);
    let mut files = vec!["shell.json", "shell.obj", "report.json"];
    files.extend(source_files.iter().map(|(name, _)| *name));
    let result = json!({
        "out":out, "source_body_sha256":shell.source_body_sha256,
        "shell_sha256":sha256_hex(shell_json.as_bytes()), "source":source_description,
        "vertices":vertices.len(), "triangles":triangles.len(),
        "boundary_rings":shell.boundary_rings.iter().map(|r| json!({
            "name":r.name,"is_start":r.is_start,"perimeter_m":r.perimeter_m,
        })).collect::<Vec<_>>(), "files":files,
    });
    let report = workspace::json_bytes(&result)?;
    // Source/geometry admission and serialization finish before publication starts.
    reserve_output(&out, "knit shell")?;
    workspace::write_new(&out.join("shell.json"), shell_json.as_bytes())?;
    workspace::write_new(&out.join("shell.obj"), obj.as_bytes())?;
    for (name, bytes) in source_files {
        workspace::write_new(&out.join(name), &bytes)?;
    }
    workspace::write_new(&out.join("report.json"), &report)?;
    Ok((
        result,
        0,
        format!(
            "KNIT SHELL: {} vertices, {} triangles, {} named openings\nwrote {}",
            vertices.len(),
            triangles.len(),
            shell.boundary_rings.len(),
            out.display()
        ),
    ))
}
