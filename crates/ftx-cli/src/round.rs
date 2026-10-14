//! Immutable workspace rounds (plan §14, spec 04 §11). The flat pattern, evidence gate and export
//! checks are independent verdict bases. A new run never reuses an older run's output directory.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use ftx_core::{Refusal, RefusalCode};
use ftx_measure::{date_day, format_round_readiness, fuse, round_readiness};
use ftx_pattern::round::{fit_contract_changes, infer_placements, round_status};
use ftx_pattern::{Measurements, Pattern, default_fit_spec, format_table, validate};
use serde_json::{Value, json};

use super::{Args, bad_input, export, known_options, make_dir, read_json};

mod report;
mod stills;

pub(super) const BOOLEAN_OPTIONS: &[&str] = &["export", "update-measurements", "sim", "stills"];

const SIM_OPTIONS: &[&str] = &[
    "sew",
    "drape",
    "elastic-ea",
    "model",
    "body-fit",
    "self-contact-mm",
    "self-mu",
    "arm-deg",
    "two-start-sew",
    "spread-vertex-mm",
    "spread-hem-mm",
    "spread-clearance-mm",
    "spread-seam-pp",
];

type SimOutcome = (Option<Value>, String, Option<String>, bool);

fn io(path: &Path, e: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display()))
}

/// Never follow a symlink at a workspace-owned output path.
fn regular(path: &Path, directory: bool) -> Result<bool, Refusal> {
    match fs::symlink_metadata(path) {
        Ok(m) if if directory { m.is_dir() } else { m.is_file() } => Ok(true),
        Ok(_) => Err(bad_input(format!(
            "{} must be a regular {}",
            path.display(),
            if directory { "directory" } else { "file" }
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(e) => Err(io(path, e)),
    }
}

pub(super) fn write_new(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path).map_err(|e| io(path, e))?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io(path, e))
}

pub(super) fn write_json(path: &Path, value: &Value) -> Result<(), Refusal> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| bad_input(e.to_string()))?;
    bytes.push(b'\n');
    write_new(path, &bytes)
}

/// Numeric directories count even when an interrupted draft did not leave a pattern.
fn inventory(ws: &Path) -> Result<(u64, Option<PathBuf>), Refusal> {
    regular(ws, true)?;
    let rounds = ws.join("rounds");
    if !regular(&rounds, true)? {
        return Ok((0, None));
    }
    let mut highest = None;
    let mut latest: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(&rounds).map_err(|e| io(&rounds, e))? {
        let entry = entry.map_err(|e| io(&rounds, e))?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        if name.is_empty() || !name.bytes().all(|b| b.is_ascii_digit()) {
            continue;
        }
        let n: u64 = name
            .parse()
            .map_err(|_| bad_input("round number is too large"))?;
        regular(&entry.path(), true)?;
        highest = Some(highest.map_or(n, |h: u64| h.max(n)));
        let pat = entry.path().join("pattern.json");
        if regular(&pat, false)? && latest.as_ref().is_none_or(|(k, _)| n > *k) {
            latest = Some((n, pat));
        }
    }
    let next = match highest {
        Some(n) => n
            .checked_add(1)
            .ok_or_else(|| bad_input("round number overflow"))?,
        None => 0,
    };
    Ok((next, latest.map(|(_, p)| p)))
}

pub(super) fn ledger(ws: &Path) -> Result<Vec<Value>, Refusal> {
    let path = ws.join("rounds.jsonl");
    if !regular(&path, false)? {
        return Ok(Vec::new());
    }
    let text = fs::read_to_string(&path).map_err(|e| io(&path, e))?;
    if !text.is_empty() && !text.ends_with('\n') {
        return Err(bad_input(
            "rounds.jsonl has an incomplete final line; repair it before adding a round",
        ));
    }
    text.lines()
        .filter(|l| !l.trim().is_empty())
        .enumerate()
        .map(|(n, l)| {
            let value: Value =
                serde_json::from_str(l).map_err(|e| io(&path, format!("line {}: {e}", n + 1)))?;
            if !value.is_object() {
                return Err(bad_input(format!(
                    "rounds.jsonl line {} is not an object",
                    n + 1
                )));
            }
            Ok(value)
        })
        .collect()
}

fn append(ws: &Path, entry: &Value) -> Result<(), Refusal> {
    let path = ws.join("rounds.jsonl");
    let mut options = OpenOptions::new();
    options.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path).map_err(|e| io(&path, e))?;
    let mut bytes = serde_json::to_vec(entry).map_err(|e| bad_input(e.to_string()))?;
    bytes.push(b'\n');
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io(&path, e))
}

/// Raw evidence is retained separately; stale fused caches never drive the verdict.
pub(super) fn measurements(raw: &Value) -> Result<Value, Refusal> {
    if !raw.is_object() {
        return Err(bad_input("measurements must be an object"));
    }
    if raw.get("records").is_some() || raw.get("schema").is_some() {
        fuse(raw, true)
    } else {
        Ok(raw.clone()) // legacy flat measurements: provenance is explicitly NOT_CHECKED
    }
}

pub(super) fn measurement_gate(pat: &Value, data: &Value, as_of: &str) -> Result<Value, Refusal> {
    let out = round_readiness(pat, data, as_of);
    if let Some(note) = out["note"].as_str()
        && note.starts_with("measurement readiness failed:")
    {
        return Err(bad_input(note));
    }
    Ok(out)
}

fn status<'a>(
    flat: &'a str,
    unscaled: bool,
    sim: Option<&Value>,
    exports: Option<&Value>,
    ready: &str,
) -> &'a str {
    round_status(flat, sim, exports, ready, unscaled)
}

/// Preflight the body change before creating a round; only explicit adoption may replace the body.
pub(super) struct WorkspaceBody {
    pub(super) raw: Value,
    pub(super) bytes: Vec<u8>,
    previous: Option<Vec<u8>>,
    changed: bool,
}

impl WorkspaceBody {
    pub(super) fn read(ws: &Path, args: &Args) -> Result<Self, Refusal> {
        let path = ws.join("measurements.json");
        let previous = if regular(&path, false)? {
            Some(fs::read(&path).map_err(|e| io(&path, e))?)
        } else {
            None
        };
        let bytes = match args.options.get("measurements") {
            Some(src) => fs::read(src).map_err(|e| io(Path::new(src), e))?,
            None => previous
                .clone()
                .ok_or_else(|| bad_input("no workspace measurements; pass --measurements M"))?,
        };
        let raw =
            serde_json::from_slice(&bytes).map_err(|e| bad_input(format!("measurements: {e}")))?;
        measurements(&raw)?;
        let changed = previous.as_ref().is_some_and(|old| *old != bytes);
        if changed && !args.options.contains_key("update-measurements") {
            return Err(bad_input(
                "workspace measurements differ. A workspace is one body: add --update-measurements to adopt the new file (the old one is kept), or start a new workspace.",
            ));
        }
        Ok(Self {
            raw,
            bytes,
            previous,
            changed,
        })
    }

    pub(super) fn adopt(&self, ws: &Path, rd: &Path, n: u64) -> Result<Vec<String>, Refusal> {
        let path = ws.join("measurements.json");
        let previous = match &self.previous {
            None => {
                write_new(&path, &self.bytes)?;
                return Ok(Vec::new());
            }
            Some(previous) => previous,
        };
        if self.changed {
            // Recheck after reservation; never replace an intervening writer's body with our stale one.
            if fs::read(&path).map_err(|e| io(&path, e))? != *previous {
                return Err(bad_input(
                    "workspace measurements changed during this run; retry with the current body",
                ));
            }
            let mut suffix = 0u64;
            let backup = loop {
                let extra = if suffix == 0 {
                    String::new()
                } else {
                    format!(".{suffix:03}")
                };
                let candidate = ws.join(format!("measurements.before-round-{n:03}{extra}.json"));
                match fs::symlink_metadata(&candidate) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => break candidate,
                    Err(e) => return Err(io(&candidate, e)),
                    Ok(_) => {}
                }
                suffix = suffix
                    .checked_add(1)
                    .ok_or_else(|| bad_input("measurement backup number overflow"))?;
            };
            write_new(&backup, previous)?;
            fs::set_permissions(
                &backup,
                fs::metadata(&path).map_err(|e| io(&path, e))?.permissions(),
            )
            .map_err(|e| io(&backup, e))?;
            let pending = rd.join("measurements.adopting.json");
            write_new(&pending, &self.bytes)?;
            fs::rename(&pending, &path).map_err(|e| io(&path, e))?;
            return Ok(vec![format!(
                "measurements updated; the previous file is kept as {}",
                backup.display()
            )]);
        }
        Ok(Vec::new())
    }
}

pub(super) fn reserve(ws: &Path) -> Result<(u64, PathBuf), Refusal> {
    let (n, _) = inventory(ws)?;
    make_dir(&ws.join("rounds"))?;
    let rd = ws.join("rounds").join(format!("{n:03}"));
    fs::create_dir(&rd).map_err(|e| io(&rd, e))?; // exclusive: another run wins rather than being overwritten
    Ok((n, rd))
}

fn fit_notes(own: &Value, contract: &Value) -> Option<String> {
    let changes = fit_contract_changes(contract, own);
    if ["new", "missing", "changed"]
        .iter()
        .all(|k| changes[k].as_array().is_some_and(Vec::is_empty))
    {
        return None;
    }
    Some(format!(
        "this draft's fit spec differs from fit_spec.json, the contract this round is judged by ({changes}); the workspace contract was not changed"
    ))
}

pub(super) fn cmd_round(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "round",
        &[
            "pattern",
            "fabric",
            "measurements",
            "fit",
            "export",
            "update-measurements",
            "note",
            "as-of",
            "created",
            "sim",
            "sew",
            "drape",
            "elastic-ea",
            "model",
            "body-fit",
            "self-contact-mm",
            "self-mu",
            "arm-deg",
            "two-start-sew",
            "spread-vertex-mm",
            "spread-hem-mm",
            "spread-clearance-mm",
            "spread-seam-pp",
            "stills",
            "res",
        ],
    )?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx round needs exactly one workspace"));
    }
    let wants_sim = args.options.contains_key("sim") || args.options.contains_key("stills");
    let render_size = if args.options.contains_key("stills") {
        Some(super::render::resolution(
            args.options.get("res").map(String::as_str),
        )?)
    } else if args.options.contains_key("res") {
        return Err(bad_input("--res requires --stills"));
    } else {
        None
    };
    if wants_sim {
        let run_options = super::drape_options(args)?;
        super::two_start::options(args, &run_options)?;
        super::drape_pose(args)?;
    } else if let Some(key) = SIM_OPTIONS.iter().find(|&&k| args.options.contains_key(k)) {
        return Err(bad_input(format!("--{key} requires --sim or --stills")));
    }
    let ws = PathBuf::from(&args.positional[1]);
    let (_, latest) = inventory(&ws)?;
    ledger(&ws)?;
    let source = args
        .options
        .get("pattern")
        .map(PathBuf::from)
        .or(latest)
        .ok_or_else(|| bad_input("workspace has no round with a pattern; pass --pattern P"))?;
    let (mut raw, _) = read_json(&source.to_string_lossy())?;
    let fabric = super::fabric::load(args)?;
    if let Some(selected) = &fabric {
        selected.apply(&mut raw)?;
    }
    let body = WorkspaceBody::read(&ws, args)?;
    let data = measurements(&body.raw)?;
    let (raw, placement_notes) = if Pattern::new(raw.clone()).structure_errors().is_empty() {
        infer_placements(&raw, &Measurements(data.clone()))?
    } else {
        (raw, Vec::new())
    };
    let pattern = Pattern::new(raw.clone());
    let at = format!("{}:00Z", export::utc_minute()?);
    let as_of = args.options.get("as-of").map_or(&at[..10], String::as_str);
    if date_day(as_of).is_none() {
        return Err(bad_input("--as-of must be YYYY-MM-DD"));
    }
    let ready = measurement_gate(&raw, &data, as_of)?;
    let own_path = args.options.get("fit").map(PathBuf::from).or_else(|| {
        source
            .parent()
            .map(|p| p.join("fit_spec.json"))
            .filter(|p| p.is_file())
    });
    let own = own_path
        .as_ref()
        .map(|p| read_json(&p.to_string_lossy()).map(|v| v.0))
        .transpose()?;
    let fp = ws.join("fit_spec.json");
    let existing_fit = regular(&fp, false)?;
    let spec = if existing_fit {
        read_json(&fp.to_string_lossy())?.0
    } else {
        own.clone().unwrap_or_else(|| {
            default_fit_spec(&pattern, raw["style"].as_str().unwrap_or("garment"))
        })
    };
    if !spec.is_object() || !spec["points"].is_array() {
        return Err(bad_input("fit spec must be an object with a points list"));
    }
    let validation = validate(&pattern, &spec, &Measurements(data.clone()));
    let unscaled = raw["meta"]["scale_status"] == "UNSCALED";
    // Share the draft command's process-scoped lock; reject evidence/contract changes during
    // preflight rather than judging a body different from the one now owning the workspace.
    let _workspace = super::workspace::Workspace::open(&ws)?;
    let current = WorkspaceBody::read(&ws, args)?;
    if current.bytes != body.bytes || current.previous != body.previous {
        return Err(bad_input(
            "workspace measurements changed during preflight; retry",
        ));
    }
    if regular(&fp, false)? != existing_fit
        || (existing_fit && read_json(&fp.to_string_lossy())?.0 != spec)
    {
        return Err(bad_input(
            "workspace fit contract changed during preflight; retry",
        ));
    }
    let (n, rd) = reserve(&ws)?;
    if let Some(selected) = &fabric {
        selected.save(&rd)?;
    }
    let mut notes = body.adopt(&ws, &rd, n)?;
    notes.extend(placement_notes);
    write_json(&rd.join("pattern.json"), &raw)?;
    write_json(&rd.join("fit_spec.json"), &spec)?;
    write_new(&rd.join("measurements.source.json"), &body.bytes)?;
    write_json(&rd.join("measurements.json"), &data)?;
    if !existing_fit {
        write_json(&fp, &spec)?;
        notes.push("created the workspace fit contract".into());
    }
    if let Some(note) = own.as_ref().and_then(|s| fit_notes(s, &spec)) {
        notes.push(note);
    }
    if unscaled {
        notes.push(
            "pattern is UNSCALED (1 unit = 1 px): establish scale before judging fit or simulating"
                .into(),
        );
    }
    write_json(&rd.join("readiness.json"), &ready)?;
    let val = serde_json::to_value(&validation).map_err(|e| bad_input(e.to_string()))?;
    write_json(&rd.join("validation.json"), &val)?;
    for (file, dark) in [("preview.svg", false), ("preview_dark.svg", true)] {
        let svg = ftx_export::diagnostic_svg(&pattern, &validation, dark)?;
        write_new(&rd.join(file), svg.as_bytes())?;
    }
    let (sim, sim_basis, viewer, budget_stop) = if wants_sim {
        simulate(
            args,
            &ws,
            &rd,
            n,
            &raw,
            &data,
            validation.status(),
            unscaled,
            &mut notes,
        )?
    } else {
        (None, "not run".to_string(), None, false)
    };
    let (rendering, images) = stills::publish(&rd, n, sim.as_ref(), render_size)?;
    let mut exports = None;
    let mut export_basis = "not run".to_string();
    if args.options.contains_key("export") {
        if validation.status() == "BLOCKED" {
            export_basis = "skipped (validation BLOCKED)".into();
        } else if unscaled {
            export_basis =
                "skipped (pattern UNSCALED: true-size files need scale-fit first)".into();
        } else {
            let out = rd.join("exports");
            let mut options =
                BTreeMap::from([("out".to_string(), out.to_string_lossy().into_owned())]);
            if let Some(v) = args.options.get("created") {
                options.insert("created".into(), v.clone());
            }
            let export_args = Args {
                positional: vec![
                    "export".into(),
                    "all".into(),
                    rd.join("pattern.json").to_string_lossy().into_owned(),
                ],
                options,
                values: Vec::new(),
                json: true,
            };
            let (result, code, _) = export::cmd_export(&export_args)?;
            let c = &result["checks"];
            export_basis = format!(
                "{}: {}/{} checks",
                c["status"].as_str().unwrap_or("FAIL"),
                c["passed"],
                c["total"]
            );
            exports = Some(
                json!({"dir":format!("rounds/{n:03}/exports"), "pass":code == 0,
                "checks":export_basis, "failures":c["failures"]}),
            );
            write_json(&out.join("checks.json"), c)?;
        }
    }
    let state = status(
        validation.status(),
        unscaled,
        sim.as_ref(),
        exports.as_ref(),
        ready["status"].as_str().unwrap_or("NOT_CHECKED"),
    );
    let basis = json!({"flat":validation.status(), "sim":sim_basis, "exports":export_basis, "measurements":ready["status"]});
    let entry = json!({"round":n, "dir":format!("rounds/{n:03}"), "note":args.options.get("note").map_or("", String::as_str),
        "auto_notes":notes, "status":state, "basis":basis, "readiness":ready, "exports":exports,
        "pattern_meta":raw["meta"], "fabric_override":fabric.as_ref().map(|f| f.receipt().clone()),
        "validation":val, "images":images, "rendering":rendering, "video":null, "viewer":viewer, "sim":sim, "at":at,
        "preview":format!("rounds/{n:03}/preview.svg"), "preview_dark":format!("rounds/{n:03}/preview_dark.svg")});
    write_json(&rd.join("round.json"), &entry)?;
    append(&ws, &entry)?;
    report::publish(&ws)?;
    let mut text = format!(
        "{}\n{}",
        format_round_readiness(&ready),
        format_table(&validation)
    );
    for note in &notes {
        text.push_str(&format!("\nNOTE: {note}"));
    }
    if let Some(sim) = &sim {
        for (key, label) in [("gating", "gating"), ("advisory", "advisory")] {
            let flags: Vec<&str> = sim[key]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .collect();
            text.push_str(&format!(
                "\nSIM {label} flags: {}",
                if flags.is_empty() {
                    "none".into()
                } else {
                    flags.join("; ")
                }
            ));
        }
    }
    text.push_str(&format!("\nROUND STATUS: {state} (flat: {}; sim: {sim_basis}; exports: {export_basis}; measurements: {})\nround dir: {}",
        validation.status(), ready["status"].as_str().unwrap_or("NOT_CHECKED"), rd.display()));
    text.push_str(&format!("\nreport: {}", ws.join("report.html").display()));
    if render_size.is_some() {
        text.push_str(&format!(
            "\nSTILLS: {}",
            rendering["status"].as_str().unwrap_or("FAILED")
        ));
    }
    let code = if budget_stop || rendering["refusal"]["code"] == "budget_exceeded" {
        4
    } else if state == "CONVERGED" && rendering["status"] != "FAILED" {
        0
    } else {
        2
    };
    Ok((entry, code, text))
}

/// Only a successful invocation in this newly reserved directory authorizes reading its report.
/// A failed solver/output write cannot substitute a previous round's report or viewer.
#[allow(clippy::too_many_arguments)]
fn simulate(
    args: &Args,
    ws: &Path,
    rd: &Path,
    number: u64,
    pattern: &Value,
    data: &Value,
    flat: &str,
    unscaled: bool,
    notes: &mut Vec<String>,
) -> Result<SimOutcome, Refusal> {
    let skipped = if flat == "BLOCKED" {
        Some("skipped (validation BLOCKED)")
    } else if unscaled {
        Some("skipped (pattern UNSCALED)")
    } else if !pattern["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .any(|p| p["sim"] != false && ftx_pattern::truthy(&p["placement"]))
    {
        Some("not applicable (no piece has a fold-up placement; judged flat)")
    } else {
        None
    };
    if let Some(why) = skipped {
        return Ok((None, why.into(), None, false));
    }
    let mut options: BTreeMap<String, String> = args
        .options
        .iter()
        .filter(|(k, _)| SIM_OPTIONS.contains(&k.as_str()))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (key, path) in [
        ("measurements", rd.join("measurements.json")),
        ("fit", rd.join("fit_spec.json")),
        ("out", rd.join("sim")),
    ] {
        options.insert(key.into(), path.to_string_lossy().into_owned());
    }
    let drape_args = Args {
        positional: vec![
            "drape".into(),
            rd.join("pattern.json").to_string_lossy().into_owned(),
        ],
        options,
        values: Vec::new(),
        json: true,
    };
    let outcome = super::cmd_drape(&drape_args).and_then(|(result, _, _)| {
        let (mut report, _) = read_json(&rd.join("sim/sim_report.json").to_string_lossy())?;
        if !report.is_object() || !report["gating"].is_array() || !report["settled"].is_boolean() {
            return Err(bad_input(
                "the new simulation did not produce a complete report",
            ));
        }
        report["advisory"] = result["advisory"].clone();
        Ok(report)
    });
    match outcome {
        Ok(mut report) => {
            let qpath = ws.join("qualification/qualify.json");
            let qdoc_owned = if regular(&qpath, false)? {
                Some(
                    read_json(&qpath.to_string_lossy())
                        .map(|(v, _)| v)
                        .map_err(|e| e.message),
                )
            } else {
                None
            };
            let qdoc_ref = qdoc_owned
                .as_ref()
                .map(|r| r.as_ref().map_err(String::as_str));
            let meas_bytes = serde_json::to_vec(data).map_err(|e| bad_input(e.to_string()))?;
            let meas_sha = ftx_core::receipt::sha256_hex(&meas_bytes);
            let qual = ftx_drape::qualify::check_qualification(
                qdoc_ref,
                &qpath.display().to_string(),
                &ws.display().to_string(),
                &meas_sha,
                pattern.get("fabric"),
            );
            if let Some(note) = ftx_drape::qualify::gate_tightness(&mut report, &qual) {
                notes.push(note);
            }
            let mut updated =
                serde_json::to_vec_pretty(&report).map_err(|e| bad_input(e.to_string()))?;
            updated.push(b'\n');
            super::workspace::replace(&rd.join("sim/sim_report.json"), &updated)?;
            let basis = if ftx_pattern::truthy(&report["gating"]) {
                "gating flags"
            } else {
                "no gating flags"
            };
            Ok((
                Some(report),
                basis.into(),
                Some(format!("rounds/{number:03}/sim/fold_viewer.html")),
                false,
            ))
        }
        Err(refusal) => {
            let budget = refusal.code == RefusalCode::BudgetExceeded;
            let flag = format!("simulation failed: {}", refusal.message);
            let report = json!({"flags":[flag], "gating":[flag], "advisory":[],
                "settled":false, "verdict":"FAILED", "refusal":refusal});
            write_json(&rd.join("sim_failure.json"), &report)?;
            Ok((
                Some(report),
                "gating flags (simulation failed)".into(),
                None,
                budget,
            ))
        }
    }
}

/// Rotates an existing workspace `qualification` directory to `qualification.before-NNN`
/// so re-running `ftx qualify --ws WS` after a body or fabric change never deletes prior evidence.
fn reserve_ws_qualification(ws: &Path) -> Result<PathBuf, Refusal> {
    let out = ws.join("qualification");
    if fs::symlink_metadata(&out).is_ok() {
        regular(&out, true)?;
        let mut suffix = 0u64;
        let archive = loop {
            let candidate = ws.join(format!("qualification.before-{suffix:03}"));
            match fs::symlink_metadata(&candidate) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => break candidate,
                Err(e) => return Err(io(&candidate, e)),
                Ok(_) => {}
            }
            suffix = suffix
                .checked_add(1)
                .ok_or_else(|| bad_input("qualification backup number overflow"))?;
        };
        fs::rename(&out, &archive).map_err(|e| io(&archive, e))?;
    }
    super::reserve_drape_output(&out)?;
    Ok(out)
}

/// `ftx qualify`: run the three known-answer chest bands (`-20`, `0`, `+20` mm ease) at zero
/// gravity (`--sew 70 --drape 0`) and write `qualify.json` (`fabric-qualify/1`, spec 04 §7.1).
pub(super) fn cmd_qualify(args: &Args) -> Result<(Value, i32, String), Refusal> {
    use ftx_body::{BodyFrames, Pose, pose_deltas, skin_mesh, trimmed_skin};
    use ftx_drape::animation::{mesh_map, pc2, seam_colours, strain_colours};
    use ftx_drape::qualify::{
        DEFAULT_BAND_HEIGHT_MM, QUALIFY_DRAPE_STEPS, QUALIFY_EASES, QUALIFY_SEW_STEPS,
        band_fit_spec, band_pattern, evaluate_band, fabric_key,
    };
    use ftx_drape::run::{DrapeBody, DrapeOptions, drape, viewer};

    known_options(
        args,
        "qualify",
        &[
            "ws",
            "measurements",
            "pattern",
            "fabric",
            "out",
            "model",
            "body-fit",
            "arm-deg",
        ],
    )?;
    if args.positional.len() > 2 || (args.options.contains_key("ws") && args.positional.len() > 1) {
        return Err(bad_input(
            "pass a workspace either with --ws WS or as a single positional argument",
        ));
    }
    let ws_arg = args
        .options
        .get("ws")
        .or_else(|| args.positional.get(1))
        .map(PathBuf::from);
    let pose = super::drape_pose(args)?;
    let fabric_override = super::fabric::load(args)?;

    let (data, mut pat, out, _ws_lock) = if let Some(ws) = ws_arg {
        if args.options.contains_key("out") {
            return Err(bad_input(
                "--out cannot be combined with a workspace (workspace qualification writes to WS/qualification)",
            ));
        }
        let lock = super::workspace::Workspace::open(&ws)?;
        let meas_path = match args.options.get("measurements") {
            Some(m) => PathBuf::from(m),
            None => ws.join("measurements.json"),
        };
        let (raw_m, _) = read_json(&meas_path.to_string_lossy())?;
        let data = measurements(&raw_m)?;
        let (_, prev_pat) = inventory(&ws)?;
        let pat = match args.options.get("pattern").map(PathBuf::from).or(prev_pat) {
            Some(p) => read_json(&p.to_string_lossy())?.0,
            None => json!({}),
        };
        let out = reserve_ws_qualification(&ws)?;
        (data, pat, out, Some(lock))
    } else {
        let meas_path = super::required(args, "measurements")?;
        let out = PathBuf::from(super::required(args, "out")?);
        let (raw_m, _) = read_json(meas_path)?;
        let data = measurements(&raw_m)?;
        let pat = match args.options.get("pattern") {
            Some(p) => read_json(p)?.0,
            None => json!({}),
        };
        super::reserve_drape_output(&out)?;
        (data, pat, out, None)
    };

    if let Some(selected) = &fabric_override {
        if !pat.is_object() {
            pat = json!({});
        }
        selected.apply(&mut pat)?;
    }

    let opts = DrapeOptions {
        sew_steps: QUALIFY_SEW_STEPS,
        drape_steps: QUALIFY_DRAPE_STEPS,
        ..DrapeOptions::default()
    };
    opts.validate()?;

    let mut bands = Vec::with_capacity(QUALIFY_EASES.len());
    for ease in QUALIFY_EASES {
        let bpat = band_pattern(&data, ease, DEFAULT_BAND_HEIGHT_MM, pat.get("fabric"))?;
        ftx_drape::material::resolve(&Pattern::new(bpat.clone()), &opts)?;
        bands.push((ease, bpat));
    }

    let super::body_source::PreparedBody {
        model,
        params,
        description,
        ..
    } = super::body_source::prepare(args, Some(&data))?;
    let rest = model.rest(&params)?;
    let posed = model.pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?)?;
    let skin = trimmed_skin(
        &model,
        &posed.vertices,
        &["wrist.R", "wrist.L", "foot.R", "foot.L"],
    )?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let frames = BodyFrames::new(&model, &posed)?;
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
    };

    let meas_bytes = serde_json::to_vec(&data).map_err(|e| bad_input(e.to_string()))?;
    let meas_sha = ftx_core::receipt::sha256_hex(&meas_bytes);
    let spec = band_fit_spec();
    let mut rows = Vec::with_capacity(bands.len());
    let mut lines = Vec::with_capacity(bands.len());

    for (ease, bpat) in bands {
        let bdir = out.join(format!("band_{ease:+}"));
        make_dir(&bdir)?;
        let pat_path = bdir.join("pattern.json");
        write_json(&pat_path, &bpat)?;
        let sim_dir = bdir.join("sim");
        super::reserve_drape_output(&sim_dir)?;
        let pattern = Pattern::new(bpat);
        let started = std::time::Instant::now();
        let run = drape(&pattern, &spec, &body, &opts, |line| eprintln!("{line}"))?;
        let mut report = run.report.clone();
        if let Some(selected) = &fabric_override {
            report["fabric_override"] = selected.receipt().clone();
        }
        if let Some(o) = report.as_object_mut() {
            o.insert("pattern".into(), json!(pat_path.to_string_lossy()));
            o.insert("measurements_sha256".into(), json!(meas_sha.clone()));
            o.insert(
                "wall_s".into(),
                json!((started.elapsed().as_secs_f64() * 10.0).round() / 10.0),
            );
            let mut b = description.clone();
            b.insert(
                "pose".into(),
                json!(format!(
                    "drape: arms at {}° and straight, hands and feet left out of contact",
                    body.arm_deg
                )),
            );
            o.insert("body".into(), Value::Object(b));
        }
        let html = viewer(&run, &pattern, &body, super::VIEWER_TEMPLATE).map_err(bad_input)?;
        let mesh = &run.scene.mesh;
        let last = run.fold.frames.last().map_or(&[][..], Vec::as_slice);
        let style = pattern.raw()["style"].as_str().unwrap_or("");
        super::write_drape_output(
            &sim_dir,
            &report,
            &[
                ("fold.pc2", pc2(&run.fold.frames)),
                (
                    "mesh_map.json",
                    serde_json::to_vec_pretty(&mesh_map(&run.scene.mesh, &run.fold))
                        .map_err(|e| bad_input(e.to_string()))?,
                ),
                ("fold_viewer.html", html.into_bytes()),
                (
                    "garment.obj",
                    super::obj(last, &mesh.faces, Some(&seam_colours(mesh, style))).into_bytes(),
                ),
                (
                    "garment_strain.obj",
                    super::obj(last, &mesh.faces, Some(&strain_colours(mesh, last))).into_bytes(),
                ),
                (
                    "body.obj",
                    super::obj(&full.vertices, &full.triangles, None).into_bytes(),
                ),
            ],
        )?;
        let (row, line, _) = evaluate_band(ease, &report);
        rows.push(row);
        lines.push(line);
    }

    let all_pass = rows.iter().all(|r| r["pass"].as_bool() == Some(true));
    let rec = json!({
        "schema": "fabric-qualify/1",
        "engine": "ftx",
        "measurements_sha256": meas_sha,
        "fabric_key": fabric_key(pat.get("fabric")),
        "bands": rows,
        "pass": all_pass,
    });
    let qfile = out.join("qualify.json");
    write_json(&qfile, &rec)?;
    let mut text = lines.join("\n");
    if all_pass {
        text.push_str(
            "\nQUALIFICATION: PASS (tightness flags may gate rounds for this body and fabric)",
        );
    } else {
        text.push_str(
            "\nQUALIFICATION: FAIL (tightness flags will stay advisory — the sim did not separate the three known-answer bands)",
        );
    }
    text.push_str(&format!("\nwrote {}", qfile.display()));
    Ok((rec, if all_pass { 0 } else { 2 }, text))
}

/// `ftx report WS`: rebuilds `WS/report.html` from the saved `rounds.jsonl` ledger without running a new round.
pub(super) fn cmd_report(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "report", &[])?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx report needs exactly one workspace"));
    }
    let ws = PathBuf::from(&args.positional[1]);
    if !regular(&ws, true)? {
        return Err(bad_input(format!(
            "{}: workspace directory does not exist",
            ws.display()
        )));
    }
    let _workspace = super::workspace::Workspace::open(&ws)?;
    let entries = ledger(&ws)?;
    report::publish(&ws)?;
    let out = ws.join("report.html");
    let text = format!("wrote {}", out.display());
    Ok((json!({"report": out, "rounds": entries.len()}), 0, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verdict_precedence_keeps_every_basis_separate() {
        let sim = json!({"gating":["crossing"]});
        let failed = json!({"pass":false});
        for (flat, scale, s, e, ready, expected) in [
            (
                "BLOCKED",
                true,
                Some(&sim),
                Some(&failed),
                "NOT_READY",
                "BLOCKED",
            ),
            (
                "NOT_CONVERGED",
                true,
                Some(&sim),
                Some(&failed),
                "NOT_READY",
                "UNSCALED",
            ),
            (
                "NOT_CONVERGED",
                false,
                Some(&sim),
                Some(&failed),
                "NOT_READY",
                "NOT_CONVERGED",
            ),
            (
                "CONVERGED",
                false,
                Some(&sim),
                Some(&failed),
                "NOT_READY",
                "SIM_GATING_FLAGS",
            ),
            (
                "CONVERGED",
                false,
                None,
                Some(&failed),
                "NOT_READY",
                "EXPORT_CHECKS_FAILED",
            ),
            (
                "CONVERGED",
                false,
                None,
                None,
                "NOT_READY",
                "MEASUREMENTS_NOT_READY",
            ),
            (
                "CONVERGED",
                false,
                None,
                None,
                "READY_REFERENCE",
                "CONVERGED",
            ),
        ] {
            assert_eq!(status(flat, scale, s, e, ready), expected);
        }
    }
}
