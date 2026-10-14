//! `ftx draft PROGRAM --measurements M --out DIR [--style S.json]`: the reference's `drafting.py` (spec 02 §C.0)
//! on `ftx_pattern::draft`.

use std::path::PathBuf;

use ftx_core::{Refusal, RefusalCode};
use ftx_export::PyJson;
use ftx_pattern::Pattern;
use ftx_pattern::draft::{PROGRAMS, draft};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir, read_json, required, workspace};

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
    let (meas, _) = read_json(required(args, "measurements")?)?;
    let style = match args.options.get("style") {
        Some(p) => Some(read_json(p)?.0),
        None => None,
    };
    let out = PathBuf::from(required(args, "out")?);
    let fabric = super::fabric::load(args)?;
    let (mut pat, spec) = draft(program, &meas, style.as_ref())?;
    if let Some(selected) = &fabric {
        selected.apply(&mut pat)?;
    }
    let (pp, sp) = (out.join("pattern.json"), out.join("fit_spec.json"));
    let errs = Pattern::new(pat.clone()).structure_errors();
    if let Some(selected) = &fabric {
        match std::fs::symlink_metadata(&out) {
            Ok(_) => {
                return Err(bad_input(
                    "draft --fabric requires a fresh output directory; existing evidence is never overwritten",
                ));
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(workspace::io_error(&out, e)),
        }
        if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
            make_dir(parent)?;
        }
        std::fs::create_dir(&out).map_err(|e| workspace::io_error(&out, e))?;
        selected.save(&out)?;
        workspace::write_new(
            &sp,
            (PyJson::from_value(&spec).dumps(Some(1)) + "\n").as_bytes(),
        )?;
        workspace::write_new(
            &pp,
            (PyJson::from_value(&pat).dumps(Some(1)) + "\n").as_bytes(),
        )?;
    } else {
        make_dir(&out)?;
        write(&pp, &pat)?;
        write(&sp, &spec)?;
    }
    let result = json!({
        "pattern": pp.display().to_string(),
        "fit_spec": sp.display().to_string(),
        "structure_errors": errs,
        "solved": pat["meta"].get("solved").cloned().unwrap_or(Value::Null),
    });
    let text = PyJson::from_value(&result).dumps(Some(1));
    Ok((result, if errs.is_empty() { 0 } else { 2 }, text))
}

/// Prepare all input and geometry before adopting a wearer or reserving a persistent round.
fn draft_workspace(args: &Args, program: &str) -> Result<(Value, i32, String), Refusal> {
    let root = PathBuf::from(required(args, "ws")?);
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
    let fabric = super::fabric::load(args)?;
    let (mut pat, spec) = draft(program, &evidence.value, style.as_ref())?;
    if let Some(selected) = &fabric {
        selected.apply(&mut pat)?;
    }
    let errs = Pattern::new(pat.clone()).structure_errors();
    let readiness = ftx_measure::round_readiness(&pat, &evidence.value, &date);
    // Do not silently repair or replace an unreadable existing workspace contract.
    let contract = root.join("fit_spec.json");
    if contract.exists() {
        workspace::read(&contract)?;
    }
    let ws = workspace::Workspace::open(&root)?;
    let note = ws.adopt(&evidence, update)?;
    let (number, dir) = ws.reserve_round()?;
    if let Some(selected) = &fabric {
        selected.save(&dir)?;
    }
    workspace::write_json(&dir.join("pattern.json"), &pat)?;
    workspace::write_json(&dir.join("fit_spec.json"), &spec)?;
    workspace::write_new(&dir.join("measurements.json"), &evidence.bytes)?;
    workspace::write_json(&dir.join("fused_measurements.json"), &evidence.value)?;
    workspace::write_json(&dir.join("readiness.json"), &readiness)?;
    if !contract.exists() {
        workspace::write_json(&contract, &spec)?;
    }
    let result = json!({"round":number,"dir":dir,"pattern":dir.join("pattern.json"),
        "fit_spec":dir.join("fit_spec.json"),"structure_errors":errs,
        "readiness":readiness,"measurement_note":note,"as_of":date,
        "solved":pat["meta"]["solved"]});
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
