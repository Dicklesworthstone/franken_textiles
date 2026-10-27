//! Evaluated GarmentCode geometry enters the ordinary native pattern workflow. Original inputs
//! are retained byte-for-byte, and the completion report is published after every other artifact.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_pattern::garmentcode_import::{MAX_JSON_BYTES, import_garmentcode_json};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, required, workspace};

fn read_bounded(path: &str) -> Result<String, Refusal> {
    let file = File::open(path).map_err(|e| workspace::io_error(Path::new(path), e))?;
    let mut bytes = Vec::new();
    file.take(MAX_JSON_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| workspace::io_error(Path::new(path), e))?;
    if bytes.len() > MAX_JSON_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("{path}: import JSON exceeds {MAX_JSON_BYTES} bytes"),
        ));
    }
    String::from_utf8(bytes).map_err(|e| bad_input(format!("{path}: {e}")))
}

pub(super) fn cmd_import(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "import garmentcode", &["out", "annotations"])?;
    if args.positional.len() != 3 || args.positional[1] != "garmentcode" {
        return Err(bad_input(
            "usage: ftx import garmentcode INPUT.json --out NEW_DIR [--annotations ANNOTATIONS.json]",
        ));
    }
    let out = Path::new(required(args, "out")?);
    match fs::symlink_metadata(out) {
        Ok(_) => {
            return Err(bad_input(format!(
                "{} already exists; choose a new import directory",
                out.display()
            )));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(workspace::io_error(out, e)),
    }
    let source = read_bounded(&args.positional[2])?;
    let annotations = args
        .options
        .get("annotations")
        .map(|path| read_bounded(path))
        .transpose()?;
    let imported = import_garmentcode_json(&source, annotations.as_deref())?;
    let pieces = imported.pattern["pieces"].as_array().map_or(0, Vec::len);
    let seams = imported.pattern["seams"].as_array().map_or(0, Vec::len);
    // Finish conversion and serialization before creating a destination. No input-supplied
    // filename is followed, and a failed write never leaves a success report behind.
    let mut files = vec![
        ("source.garmentcode.json", source.into_bytes()),
        ("pattern.json", workspace::json_bytes(&imported.pattern)?),
    ];
    if let Some(annotations) = annotations {
        files.push(("annotations.json", annotations.into_bytes()));
    }
    if let Some(fit) = imported.report.get("fit_spec").filter(|v| !v.is_null()) {
        files.push(("fit_spec.json", workspace::json_bytes(fit)?));
    }
    files.push(("import.json", workspace::json_bytes(&imported.report)?));
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|e| workspace::io_error(parent, e))?;
    }
    fs::create_dir(out).map_err(|e| workspace::io_error(out, e))?;
    for (name, bytes) in &files {
        workspace::write_new(&out.join(name), bytes)?;
    }
    Ok((
        json!({"out": out, "status": "REVIEW_REQUIRED", "pieces": pieces,
            "seams": seams, "report": imported.report}),
        0,
        format!(
            "imported {pieces} physical panels and {seams} declared stitches -> {}\nREVIEW_REQUIRED: review manufacturing annotations, then run explicit flat-fit and drape checks; conversion is not a fit verdict",
            out.display()
        ),
    ))
}
