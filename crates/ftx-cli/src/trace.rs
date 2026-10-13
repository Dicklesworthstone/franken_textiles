//! `ftx annotate`, `ftx scale-fit` and `ftx audit`: the reference's `vectorize.py` commands on a traced draft
//! (spec 04 §10.1), on `ftx-trace`. Documents are read as text so vertex and `names.json` order survive.

use std::path::Path;

use ftx_core::{Refusal, RefusalCode, py};
use ftx_pattern::doc::PatternDoc;
use ftx_trace::{AuditOptions, OValue, annotate, audit, fit_scale, parse_length};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir, required};

fn read_text(path: &str) -> Result<String, Refusal> {
    std::fs::read_to_string(path).map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))
}

fn read_doc(path: &str) -> Result<PatternDoc, Refusal> {
    serde_json::from_str(&read_text(path)?).map_err(|e| bad_input(format!("{path}: {e}")))
}

fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), Refusal> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        make_dir(dir)?;
    }
    let text = serde_json::to_string_pretty(value).map_err(|e| bad_input(e.to_string()))?;
    std::fs::write(path, text + "\n")
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))
}

fn input<'a>(args: &'a Args, command: &str, what: &str) -> Result<&'a str, Refusal> {
    args.positional
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| bad_input(format!("ftx {command} needs {what}")))
}

/// `ftx annotate DRAFT --names N --out P`: `names.json` applied to a traced draft; the result passes the
/// structure check. Prints the output path, the piece and seam ids and the annotation notes.
pub(super) fn cmd_annotate(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "annotate", &["names", "out"])?;
    let draft = read_doc(input(args, "annotate", "a traced draft")?)?;
    let names_path = required(args, "names")?;
    let out = required(args, "out")?;
    let names: OValue = serde_json::from_str(&read_text(names_path)?)
        .map_err(|e| bad_input(format!("{names_path}: {e}")))?;
    let data = annotate(&draft, &names).map_err(bad_input)?;
    write_json(Path::new(out), &data)?;
    let pieces: Vec<&str> = data.pieces.iter().map(|p| p.id.as_str()).collect();
    let seams: Vec<String> = data
        .seams()
        .iter()
        .map(|s| s.id().map_or_else(|| s.id_text(), str::to_string))
        .collect();
    let notes = data
        .meta
        .as_ref()
        .and_then(|m| m.get("annotated"))
        .and_then(|a| a.get("notes"))
        .cloned()
        .unwrap_or_else(|| json!([]));
    let mut text = format!(
        "annotated {} pieces ({}) and {} seams -> {out}",
        pieces.len(),
        pieces.join(", "),
        seams.len()
    );
    for n in notes
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
    {
        text.push_str("\n  note: ");
        text.push_str(n);
    }
    Ok((
        json!({"out": out, "pieces": pieces, "seams": seams, "notes": notes}),
        0,
        text,
    ))
}

/// `REF=LEN` (units allowed, millimetres by default).
fn target(s: &str) -> Result<(String, f64), Refusal> {
    let Some((r, val)) = s.rsplit_once('=') else {
        return Err(bad_input(format!(
            "target {} must be REF=MM",
            py::repr_str(s)
        )));
    };
    let mm = parse_length(val, Some("mm")).ok_or_else(|| {
        bad_input(format!(
            "target {}: cannot parse length {}",
            py::repr_str(s),
            py::repr_str(val)
        ))
    })?;
    Ok((r.trim().to_string(), mm))
}

/// `ftx scale-fit PATTERN --target REF=LEN [--target …] --out S`: the least-squares uniform scale from known
/// lengths; writes the FITTED pattern and prints the report.
pub(super) fn cmd_scale_fit(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "scale-fit", &["target", "out"])?;
    let doc = read_doc(input(args, "scale-fit", "a pattern")?)?;
    let out = required(args, "out")?;
    let Some(given) = args.options.get("target") else {
        return Err(bad_input("scale-fit needs at least one --target REF=MM"));
    };
    let targets = given
        .split('\n')
        .map(target)
        .collect::<Result<Vec<_>, _>>()?;
    let (data, report) =
        fit_scale(&doc, &targets).map_err(|e| bad_input(format!("scale-fit failed: {e}")))?;
    write_json(Path::new(out), &data)?;
    let mut result = json!({"out": out});
    if let (Some(r), Some(rep)) = (result.as_object_mut(), report.as_object()) {
        r.extend(rep.clone());
    }
    let mut text = format!(
        "k = {} ({} mm per source px), rms {} % -> {out}",
        py::repr_f64(report["k"].as_f64().unwrap_or(f64::NAN)),
        report["mm_per_px"]
            .as_f64()
            .map_or_else(|| "?".into(), |x| format!("{x:.4}")),
        report["rms_pct"]
    );
    if let Some(w) = report["warning"].as_str() {
        text.push_str("\n  warning: ");
        text.push_str(w);
    }
    Ok((result, 0, text))
}

fn positive(args: &Args, key: &str, default: f64) -> Result<f64, Refusal> {
    match args.options.get(key) {
        None => Ok(default),
        Some(v) => match v.parse::<f64>() {
            Ok(x) if x.is_finite() && x > 0.0 => Ok(x),
            _ => Err(Refusal::new(
                RefusalCode::OutOfRange,
                format!("--{key} must be a positive number, got {v}"),
            )),
        },
    }
}

/// `ftx audit PATTERN --out DIR [--labels L] [--tol-mm 3] [--median-seam-mm 250] [--label-tol 0.03]`: the
/// scale-free round-0 audit, written as `DIR/round0_audit.json`. Exits 2 when a finding is an error.
pub(super) fn cmd_audit(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "audit",
        &["out", "labels", "tol-mm", "median-seam-mm", "label-tol"],
    )?;
    let doc = read_doc(input(args, "audit", "a pattern")?)?;
    let out = Path::new(required(args, "out")?);
    let labels: Option<Vec<Value>> = match args.options.get("labels") {
        None => None,
        Some(p) => Some(
            serde_json::from_str(&read_text(p)?)
                .map_err(|e| bad_input(format!("{p}: a JSON list of labels expected ({e})")))?,
        ),
    };
    let opts = AuditOptions {
        tol_mm: positive(args, "tol-mm", 3.0)?,
        median_seam_mm: positive(args, "median-seam-mm", 250.0)?,
        label_tol: positive(args, "label-tol", 0.03)?,
        ..AuditOptions::default()
    };
    let report = audit(&doc, labels.as_deref(), &opts);
    make_dir(out)?;
    let file = out.join("round0_audit.json");
    write_json(&file, &report)?;
    let findings: Vec<String> = report["findings"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|f| {
            format!(
                "{} [{}] {}",
                f["id"].as_str().unwrap_or(""),
                f["class"].as_str().unwrap_or(""),
                f["message"].as_str().unwrap_or("")
            )
        })
        .collect();
    let ok = report["summary"]["ok"].as_bool().unwrap_or(false);
    let mut text = format!(
        "round-0 audit: T {} S {} M {} ({}), scale basis {}",
        report["summary"]["T"],
        report["summary"]["S"],
        report["summary"]["M"],
        if ok { "ok" } else { "errors" },
        report["scale_basis"]["kind"].as_str().unwrap_or("?")
    );
    for f in &findings {
        text.push_str("\n  ");
        text.push_str(f);
    }
    let result = json!({"summary": report["summary"], "scale_basis": report["scale_basis"],
                        "findings": findings, "files": [file.display().to_string()]});
    Ok((result, if ok { 0 } else { 2 }, text))
}
