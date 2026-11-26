//! `ftx draft PROGRAM --measurements M --out DIR [--style S.json]`: the reference's `drafting.py` (spec 02 §C.0)
//! on `ftx_pattern::draft`.

use std::path::PathBuf;

use ftx_core::{Refusal, RefusalCode};
use ftx_export::PyJson;
use ftx_pattern::Pattern;
use ftx_pattern::draft::{PROGRAMS, draft};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir, read_json, required, workspace};

struct BodyDraftEvidence {
    body: Value,
    validation: Value,
    source: Option<Vec<u8>>,
}

fn prepare(
    args: &Args,
    program: &str,
    measurements: &Value,
    style: Option<&Value>,
) -> Result<(Value, Value, Option<BodyDraftEvidence>), Refusal> {
    if !args.options.contains_key("body-fit") && !args.options.contains_key("model") {
        let (pattern, spec) = draft(program, measurements, style)?;
        return Ok((pattern, spec, None));
    }
    let (model, hash, source) = super::body_source::selected_inputs(args)?;
    let text = source
        .as_deref()
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|e| bad_input(e.to_string()))?;
    let drafted = ftx_drape::drafting::draft_on_selected_model(
        &model,
        &hash,
        program,
        measurements,
        text,
        style,
    )?;
    Ok((
        drafted.pattern,
        drafted.fit_spec,
        Some(BodyDraftEvidence {
            body: drafted.body,
            validation: drafted.validation,
            source,
        }),
    ))
}

fn save_body(dir: &std::path::Path, evidence: &BodyDraftEvidence) -> Result<(), Refusal> {
    if let Some(source) = &evidence.source {
        workspace::write_new(&dir.join("body_fit.json"), source)?;
    }
    workspace::write_json(&dir.join("draft_body.json"), &evidence.body)?;
    workspace::write_json(&dir.join("validation_body.json"), &evidence.body)?;
    workspace::write_json(&dir.join("validation.json"), &evidence.validation)
}

fn write(path: &PathBuf, v: &Value) -> Result<(), Refusal> {
    std::fs::write(path, PyJson::from_value(v).dumps(Some(1)) + "\n")
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))
}

/// Writes `pattern.json` and `fit_spec.json` into `--out` (replacing them, as the reference does) and prints
/// `{pattern, fit_spec, structure_errors, solved}`; exit 0, or 2 with structure errors (the reference exits 1).
pub(super) fn cmd_draft(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "draft",
        &[
            "measurements",
            "style",
            "out",
            "ws",
            "update-measurements",
            "as-of",
            "no-priors",
            "fabric",
            "material-bindings",
            "body-fit",
            "model",
        ],
    )?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx draft needs exactly one program")
            .repair(format!("programs: {}", PROGRAMS.join(", "))));
    }
    let program = &args.positional[1];
    if !PROGRAMS.contains(&program.as_str()) {
        return Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown program {program:?}"),
        )
        .repair(format!("programs: {}", PROGRAMS.join(", "))));
    }
    if args.options.contains_key("ws") {
        if args.options.contains_key("out") {
            return Err(bad_input("use --ws or --out, not both"));
        }
        return draft_workspace(args, program);
    }
    for key in ["update-measurements", "as-of", "no-priors"] {
        if args.options.contains_key(key) {
            return Err(bad_input(format!("--{key} requires --ws")));
        }
    }
    let (raw, _) = read_json(required(args, "measurements")?)?;
    let meas = super::current_measurements(raw)?;
    let style = match args.options.get("style") {
        Some(p) => Some(read_json(p)?.0),
        None => None,
    };
    let out = PathBuf::from(required(args, "out")?);
    let input = super::fabric::load(args)?;
    let (mut pat, spec, body) = prepare(args, program, &meas, style.as_ref())?;
    let fabric = input.map(|input| input.apply(&mut pat)).transpose()?;
    if fabric.is_some() {
        super::fabric::snapshot_bytes(&pat, "effective draft pattern")?;
    }
    let (pp, sp) = (out.join("pattern.json"), out.join("fit_spec.json"));
    let errs = Pattern::new(pat.clone()).structure_errors();
    if fabric.is_some() || body.is_some() {
        match crate::round::symlink_metadata(&out) {
            Ok(_) => {
                return Err(bad_input(
                    "draft with a selected fabric or body requires a fresh output directory; existing evidence is never overwritten",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(workspace::io_error(&out, e)),
        }
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            make_dir(parent)?;
        }
        std::fs::create_dir(&out).map_err(|e| workspace::io_error(&out, e))?;
        if let Some(selected) = &fabric {
            selected.save(&out)?;
        }
        if let Some(evidence) = &body {
            save_body(&out, evidence)?;
        }
        workspace::write_new(
            &sp,
            (PyJson::from_value(&spec).dumps(Some(1)) + "\n").as_bytes(),
        )?;
        workspace::write_new(
            &pp,
            (PyJson::from_value(&pat).dumps(Some(1)) + "\n").as_bytes(),
        )?;
    } else {
        crate::round::regular(&out, true)?;
        crate::round::regular(&pp, false)?;
        crate::round::regular(&sp, false)?;
        make_dir(&out)?;
        write(&pp, &pat)?;
        write(&sp, &spec)?;
    }
    let mut result = json!({
        "pattern": pp.display().to_string(),
        "fit_spec": sp.display().to_string(),
        "structure_errors": errs,
        "solved": pat["meta"].get("solved").cloned().unwrap_or(Value::Null),
    });
    if let Some(selected) = &fabric {
        result["fabric_override"] = selected.receipt().clone();
    }
    if let Some(evidence) = &body {
        result["draft_body"] = json!(out.join("draft_body.json"));
        result["validation_body"] = json!(out.join("validation_body.json"));
        if evidence.source.is_some() {
            result["body_fit"] = json!(out.join("body_fit.json"));
        }
        result["validation"] = json!(out.join("validation.json"));
    }
    let text = PyJson::from_value(&result).dumps(Some(1));
    Ok((result, if errs.is_empty() { 0 } else { 2 }, text))
}

/// Prepare all input and geometry before adopting a wearer or reserving a persistent round.
fn draft_workspace(args: &Args, program: &str) -> Result<(Value, i32, String), Refusal> {
    let root = PathBuf::from(required(args, "ws")?);
    crate::round::regular(&root, true)?;
    let date = super::measure::date(args, "as-of")?;
    let evidence = workspace::evidence(required(args, "measurements")?, args)?;
    let update = args.options.contains_key("update-measurements");
    workspace::check_adoption(&root, &evidence.bytes, update)?;
    workspace::next_round(&root)?;
    let style = args
        .options
        .get("style")
        .map(|p| read_json(p).map(|x| x.0))
        .transpose()?;
    let input = super::fabric::load(args)?;
    let (mut pat, spec, body) = prepare(args, program, &evidence.value, style.as_ref())?;
    let fabric = input.map(|input| input.apply(&mut pat)).transpose()?;
    if fabric.is_some() {
        super::fabric::snapshot_bytes(&pat, "effective draft pattern")?;
    }
    let errs = Pattern::new(pat.clone()).structure_errors();
    let readiness = ftx_measure::round_readiness(&pat, &evidence.value, &date);
    // Do not silently repair or replace an unreadable existing workspace contract.
    let contract = root.join("fit_spec.json");
    if crate::round::regular(&contract, false)? {
        workspace::read(&contract)?;
    }
    let ws = workspace::Workspace::open(&root)?;
    let note = ws.adopt(&evidence, update)?;
    let (number, dir) = ws.reserve_round()?;
    if let Some(selected) = &fabric {
        selected.save(&dir)?;
    }
    if let Some(evidence) = &body {
        save_body(&dir, evidence)?;
    }
    workspace::write_json(&dir.join("pattern.json"), &pat)?;
    workspace::write_json(&dir.join("fit_spec.json"), &spec)?;
    workspace::write_new(&dir.join("measurements.json"), &evidence.bytes)?;
    workspace::write_json(&dir.join("fused_measurements.json"), &evidence.value)?;
    workspace::write_json(&dir.join("readiness.json"), &readiness)?;
    if !crate::round::regular(&contract, false)? {
        workspace::write_json(&contract, &spec)?;
    }
    let mut result = json!({"round":number,"dir":dir,"pattern":dir.join("pattern.json"),
        "fit_spec":dir.join("fit_spec.json"),"structure_errors":errs,
        "readiness":readiness,"measurement_note":note,"as_of":date,
        "solved":pat["meta"]["solved"]});
    if let Some(selected) = &fabric {
        result["fabric_override"] = selected.receipt().clone();
    }
    if let Some(evidence) = &body {
        result["draft_body"] = json!(dir.join("draft_body.json"));
        result["validation_body"] = json!(dir.join("validation_body.json"));
        if evidence.source.is_some() {
            result["body_fit"] = json!(dir.join("body_fit.json"));
        }
        result["validation"] = json!(dir.join("validation.json"));
    }
    let text = format!(
        "{}{}\nround dir: {}{}",
        note.map_or(String::new(), |s| format!("NOTE: {s}\n")),
        ftx_measure::format_round_readiness(&readiness),
        dir.display(),
        if errs.is_empty() {
            String::new()
        } else {
            format!("\nWARNING: {}", errs.join("; "))
        }
    );
    Ok((result, if errs.is_empty() { 0 } else { 2 }, text))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    #[test]
    fn draft_refuses_symlinked_outputs_and_workspace_links() {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ftx-draft-symlink-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let meas = ftx_measure::reference_wearer(920.0, Some(30.0), None, None).unwrap();
        let meas_path = root.join("m.json");
        std::fs::write(&meas_path, serde_json::to_vec(&meas).unwrap()).unwrap();

        let real_out = root.join("real_out");
        std::fs::create_dir(&real_out).unwrap();
        let linked_out = root.join("linked_out");
        symlink(&real_out, &linked_out).unwrap();
        let args = crate::parse(&[
            "draft".into(),
            "fez".into(),
            "--measurements".into(),
            meas_path.to_string_lossy().into_owned(),
            "--out".into(),
            linked_out.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let err = cmd_draft(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(std::fs::read_dir(&real_out).unwrap().next().is_none());

        let out_dir = root.join("out_file_link");
        std::fs::create_dir(&out_dir).unwrap();
        let victim = root.join("victim.json");
        std::fs::write(&victim, b"untouched").unwrap();
        symlink(&victim, out_dir.join("fit_spec.json")).unwrap();
        let args = crate::parse(&[
            "draft".into(),
            "fez".into(),
            "--measurements".into(),
            meas_path.to_string_lossy().into_owned(),
            "--out".into(),
            out_dir.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let err = cmd_draft(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        assert!(
            !out_dir.join("pattern.json").exists(),
            "pattern.json must not be written when fit_spec.json is a symlink"
        );

        let real_ws = root.join("real_ws");
        std::fs::create_dir(&real_ws).unwrap();
        let linked_ws = root.join("linked_ws");
        symlink(&real_ws, &linked_ws).unwrap();
        let args = crate::parse(&[
            "draft".into(),
            "fez".into(),
            "--measurements".into(),
            meas_path.to_string_lossy().into_owned(),
            "--ws".into(),
            linked_ws.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let err = cmd_draft(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(std::fs::read_dir(&real_ws).unwrap().next().is_none());

        let ws_fit_link = root.join("ws_fit_link");
        std::fs::create_dir(&ws_fit_link).unwrap();
        symlink(&victim, ws_fit_link.join("fit_spec.json")).unwrap();
        let args = crate::parse(&[
            "draft".into(),
            "fez".into(),
            "--measurements".into(),
            meas_path.to_string_lossy().into_owned(),
            "--ws".into(),
            ws_fit_link.to_string_lossy().into_owned(),
        ])
        .unwrap();
        let err = cmd_draft(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("regular file"), "{}", err.message);
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        assert!(!ws_fit_link.join("rounds").exists());
    }
}
