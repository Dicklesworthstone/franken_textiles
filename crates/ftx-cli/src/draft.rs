//! `ftx draft PROGRAM --measurements M --out DIR [--style S.json]`: the reference's `drafting.py` (spec 02 §C.0)
//! on `ftx_pattern::draft`.

use std::path::PathBuf;

use ftx_core::{Refusal, RefusalCode};
use ftx_export::PyJson;
use ftx_pattern::Pattern;
use ftx_pattern::draft::{PROGRAMS, draft};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir, read_json, required};

fn write(path: &PathBuf, v: &Value) -> Result<(), Refusal> {
    std::fs::write(path, PyJson::from_value(v).dumps(Some(1)) + "\n")
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))
}

/// Writes `pattern.json` and `fit_spec.json` into `--out` (replacing them, as the reference does) and prints
/// `{pattern, fit_spec, structure_errors, solved}`; exit 0, or 2 with structure errors (the reference exits 1).
pub(super) fn cmd_draft(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "draft", &["measurements", "style", "out"])?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx draft needs exactly one program")
            .repair(format!("programs: {}", PROGRAMS.join(", "))));
    }
    let program = &args.positional[1];
    let (meas, _) = read_json(required(args, "measurements")?)?;
    let style = match args.options.get("style") {
        Some(p) => Some(read_json(p)?.0),
        None => None,
    };
    let out = PathBuf::from(required(args, "out")?);
    let (pat, spec) = draft(program, &meas, style.as_ref())?;
    make_dir(&out)?;
    let (pp, sp) = (out.join("pattern.json"), out.join("fit_spec.json"));
    let errs = Pattern::new(pat.clone()).structure_errors();
    write(&pp, &pat)?;
    write(&sp, &spec)?;
    let result = json!({
        "pattern": pp.display().to_string(),
        "fit_spec": sp.display().to_string(),
        "structure_errors": errs,
        "solved": pat["meta"].get("solved").cloned().unwrap_or(Value::Null),
    });
    let text = PyJson::from_value(&result).dumps(Some(1));
    Ok((result, if errs.is_empty() { 0 } else { 2 }, text))
}
