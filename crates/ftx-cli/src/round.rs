//! Immutable workspace rounds (plan §14, spec 04 §11). The flat pattern, evidence gate and export
//! checks are independent verdict bases. A new run never reuses an older run's output directory.

use std::collections::BTreeMap;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use ftx_core::{Refusal, RefusalCode};
use ftx_measure::{date_day, format_round_readiness, fuse, round_readiness};
use ftx_pattern::round::{fit_contract_changes, infer_placements, round_status};
use ftx_pattern::{BodyMeasure, Measurements, Pattern, default_fit_spec, format_table, validate};
use serde_json::{Value, json};

use super::{Args, bad_input, export, known_options, make_dir, read_json};

mod report;
mod stills;

pub(super) const BOOLEAN_OPTIONS: &[&str] = &[
    "export",
    "update-measurements",
    "sim",
    "stills",
    "flat-body",
];

const SIM_OPTIONS: &[&str] = &[
    "sew",
    "drape",
    "time-step-s",
    "newton-tol-m-s",
    "spacing-mm",
    "compression",
    "max-settle-steps",
    "static-equilibrium",
    "static-tol-mm",
    "static-max-newton",
    "time-subdivision",
    "subdivision-max-halvings",
    "subdivision-max-trials",
    "newton-strategy",
    "integrator",
    "rho-inf",
    "bending-model",
    "compression-model",
    "air-drag",
    "air-cn",
    "air-ct",
    "air-rho",
    "air-velocity",
    "rayleigh-membrane-s",
    "rayleigh-bending-s",
    "adaptive-barrier",
    "stick-slip-mus",
    "stick-slip-muk",
    "stick-slip-v",
    "self-stick-slip-mus",
    "self-stick-slip-muk",
    "self-stick-slip-v",
    "min-spacing-mm",
    "cvt-iters",
    "max-turning-angle-rad",
    "gather-refinement",
    "construction-standard",
    "hem-width-mm",
    "hem-mass-mult",
    "hem-bending-mult",
    "seam-width-mm",
    "seam-mass-mult",
    "seam-bending-mult",
    "start-self-repulsion",
    "start-repulsion-mm",
    "start-repulsion-stiffness",
    "start-repulsion-period",
    "start-repulsion-max-checks",
    "elastic-ea",
    "elastic-hold-steps",
    "model",
    "body-fit",
    "self-contact-mm",
    "self-mu",
    "arm-deg",
    "left-arm-deg",
    "right-arm-deg",
    "thumb-pose",
    "legs",
    "leg-fraction",
    "skinning",
    "hands",
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
pub(super) fn regular(path: &Path, directory: bool) -> Result<bool, Refusal> {
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
        fuse(raw, true).map_err(|refusal| {
            // The commonest way here: a document that carries only a `fused` block, which `ftx drape`
            // and `ftx place` take as explicit targets. Say how to get from one to the other.
            if raw.get("records").is_some_and(Value::is_array) {
                refusal
            } else {
                refusal.repair(
                    "This command recomputes every fused value from the original readings and never \
                     trusts a saved `fused` block, so the file needs a `records` list. `ftx measure init \
                     --label NAME --out M.json` starts one and `ftx measure add M.json --name chest \
                     --value 52cm` appends a reading; `ftx measure reference --stature-mm MM --out \
                     M.json` writes a chart-reference wearer. `ftx drape` and `ftx place` accept explicit \
                     targets without records.",
                )
            }
        })
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
            "material-bindings",
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
            "time-step-s",
            "newton-tol-m-s",
            "spacing-mm",
            "compression",
            "max-settle-steps",
            "static-equilibrium",
            "static-tol-mm",
            "static-max-newton",
            "time-subdivision",
            "subdivision-max-halvings",
            "subdivision-max-trials",
            "newton-strategy",
            "integrator",
            "rho-inf",
            "bending-model",
            "compression-model",
            "air-drag",
            "air-cn",
            "air-ct",
            "air-rho",
            "air-velocity",
            "rayleigh-membrane-s",
            "rayleigh-bending-s",
            "adaptive-barrier",
            "stick-slip-mus",
            "stick-slip-muk",
            "stick-slip-v",
            "self-stick-slip-mus",
            "self-stick-slip-muk",
            "self-stick-slip-v",
            "min-spacing-mm",
            "cvt-iters",
            "max-turning-angle-rad",
            "gather-refinement",
            "construction-standard",
            "hem-width-mm",
            "hem-mass-mult",
            "hem-bending-mult",
            "seam-width-mm",
            "seam-mass-mult",
            "seam-bending-mult",
            "start-self-repulsion",
            "start-repulsion-mm",
            "start-repulsion-stiffness",
            "start-repulsion-period",
            "start-repulsion-max-checks",
            "elastic-ea",
            "elastic-hold-steps",
            "model",
            "body-fit",
            "flat-body",
            "self-contact-mm",
            "self-mu",
            "arm-deg",
            "left-arm-deg",
            "right-arm-deg",
            "thumb-pose",
            "legs",
            "leg-fraction",
            "skinning",
            "hands",
            "two-start-sew",
            "spread-vertex-mm",
            "spread-hem-mm",
            "spread-clearance-mm",
            "spread-seam-pp",
            "stills",
            "res",
            "shading",
        ],
    )?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx round needs exactly one workspace"));
    }
    let wants_sim = args.options.contains_key("sim") || args.options.contains_key("stills");
    let (render_size, render_shading) = if args.options.contains_key("stills") {
        (
            Some(super::render::resolution(
                args.options.get("res").map(String::as_str),
            )?),
            stills::RoundShading::parse(args.options.get("shading").map(String::as_str))?,
        )
    } else if args.options.contains_key("res") {
        return Err(bad_input("--res requires --stills"));
    } else if args.options.contains_key("shading") {
        return Err(bad_input("--shading requires --stills"));
    } else {
        (None, stills::RoundShading::Standard)
    };
    if wants_sim {
        let run_options = super::drape_options(args)?;
        super::two_start::options(args, &run_options)?;
        super::drape_pose(args)?;
        super::parse_thumb_pose(args)?;
        super::parse_skinning(args)?;
        super::parse_hands(args)?;
    } else if let Some(key) = SIM_OPTIONS
        .iter()
        .find(|&&k| !matches!(k, "model" | "body-fit") && args.options.contains_key(k))
    {
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
    let fabric = super::fabric::load(args)?
        .map(|input| input.apply(&mut raw))
        .transpose()?;
    if fabric.is_some() {
        super::fabric::snapshot_bytes(&raw, "effective round pattern")?;
    }
    let body = WorkspaceBody::read(&ws, args)?;
    let data = measurements(&body.raw)?;
    let drafted_on_body = raw["meta"].get("draft_body").is_some();
    let supplied_body = args.options.contains_key("model") || args.options.contains_key("body-fit");
    // Existing simulation-only source options keep their flat measurement authority. Explicit
    // flat selection, or continuation of a model-derived draft, uses one selected body instead.
    let selected_flat = args.options.contains_key("flat-body")
        || (supplied_body && (!wants_sim || drafted_on_body));
    let (raw, mut placement_notes) = if !selected_flat
        && !drafted_on_body
        && Pattern::new(raw.clone()).structure_errors().is_empty()
    {
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
    let selected = super::selected_flat_body(args, &raw, &data, &spec, selected_flat)?;
    let raw = if let Some(selected) = &selected
        && pattern.structure_errors().is_empty()
    {
        let (placed, notes) = infer_placements(&raw, &selected.body)?;
        placement_notes.extend(notes);
        placed
    } else {
        raw
    };
    let pattern = Pattern::new(raw.clone());
    let validation = if let Some(selected) = &selected {
        validate(&pattern, &spec, &selected.body)
    } else {
        validate(&pattern, &spec, &Measurements(data.clone()))
    };
    let validation_body = selected.as_ref().map(|selected| selected.body.evidence());
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
    if wants_sim {
        recover_ws_qualification(&ws)?;
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
    if let Some(selected) = &selected {
        write_json(
            &rd.join("validation_body.json"),
            validation_body.as_ref().unwrap(),
        )?;
        if let Some(source) = &selected.source {
            write_new(&rd.join("body_fit.json"), source)?;
        }
        notes.push("flat validation uses MODEL_FIT dimensions and landmarks in the source measurement pose".into());
    }
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
            &body.bytes,
            validation.status(),
            unscaled,
            selected
                .as_ref()
                .and_then(|selected| selected.source.as_ref())
                .is_some(),
            validation_body
                .as_ref()
                .and_then(|body| body["identity"]["reference"]["model_sha256"].as_str()),
            &mut notes,
        )?
    } else {
        (None, "not run".to_string(), None, false)
    };
    let (rendering, images) = stills::publish(&rd, n, sim.as_ref(), render_size, render_shading)?;
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
    let mut basis = json!({"flat":validation.status(), "sim":sim_basis, "exports":export_basis, "measurements":ready["status"]});
    if selected.is_some() {
        basis["flat_body"] = json!("MODEL_FIT");
    }
    let mut entry = json!({"round":n, "dir":format!("rounds/{n:03}"), "note":args.options.get("note").map_or("", String::as_str),
        "auto_notes":notes, "status":state, "basis":basis, "readiness":ready, "exports":exports,
        "pattern_meta":raw["meta"], "fabric_override":fabric.as_ref().map(|f| f.receipt().clone()),
        "validation":val, "images":images, "rendering":rendering, "video":null, "viewer":viewer, "sim":sim, "at":at,
        "preview":format!("rounds/{n:03}/preview.svg"), "preview_dark":format!("rounds/{n:03}/preview_dark.svg")});
    if let Some(body) = validation_body {
        entry["validation_body"] = body;
    }
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
        append_round_sim_text(&mut text, sim);
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

/// Format round-level simulation flags and any opt-in drape diagnostics without changing default output.
fn append_round_sim_text(text: &mut String, sim: &Value) {
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
    let mut diagnostics = String::new();
    super::append_drape_constitutive_summary(sim, Some(&mut diagnostics), None);
    for line in diagnostics.lines() {
        text.push_str(&format!("\nSIM {line}"));
    }
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
    measurements_file: &[u8],
    flat: &str,
    unscaled: bool,
    retained_body_fit: bool,
    expected_model_sha256: Option<&str>,
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
    if retained_body_fit {
        // Flat validation and simulation must consume the same accepted report, even if the
        // caller replaces the external report after this round's preflight.
        options.insert(
            "body-fit".into(),
            rd.join("body_fit.json").to_string_lossy().into_owned(),
        );
    }
    for (key, path) in [
        ("measurements", rd.join("measurements.json")),
        ("fit", rd.join("fit_spec.json")),
        ("out", rd.join("sim")),
    ] {
        options.insert(key.into(), path.to_string_lossy().into_owned());
    }
    // A refused drape draws its own pictures. They are the size of this round's stills, so a pixel picked
    // in a round that completed names the same place in one that was refused.
    if let Some(res) = args.options.get("res") {
        options.insert("res".into(), res.clone());
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
    let outcome =
        super::cmd_drape_bound(&drape_args, expected_model_sha256).and_then(|(result, _, _)| {
            let (mut report, _) = read_json(&rd.join("sim/sim_report.json").to_string_lossy())?;
            if !report.is_object()
                || !report["gating"].is_array()
                || !report["settled"].is_boolean()
            {
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
            let fit_spec_owned = read_json(&rd.join("fit_spec.json").to_string_lossy())
                .ok()
                .map(|(v, _)| v);
            // Native qualification also binds the actual posed collider, resolved material(s),
            // and collider-vs-target landmark girth residuals.
            let meas_sha = ftx_core::receipt::sha256_hex(measurements_file);
            let qual = ftx_drape::qualify::check_qualification(
                qdoc_ref,
                &qpath.display().to_string(),
                &ws.display().to_string(),
                &meas_sha,
                pattern.get("fabric"),
                &report["qualification_basis"],
            );
            if let Some(note) = ftx_drape::qualify::gate_tightness_with_spec(
                &mut report,
                &qual,
                fit_spec_owned.as_ref(),
            ) {
                notes.push(note);
            }
            let mut updated =
                serde_json::to_vec_pretty(&report).map_err(|e| bad_input(e.to_string()))?;
            updated.push(b'\n');
            super::workspace::replace(&rd.join("sim/sim_report.json"), &updated)?;
            let viewer_path = rd.join("sim/fold_viewer.html");
            if regular(&viewer_path, false)? {
                let raw_html = fs::read_to_string(&viewer_path).map_err(|e| io(&viewer_path, e))?;
                let synced = ftx_drape::animation::sync_viewer_report(
                    super::VIEWER_TEMPLATE,
                    &raw_html,
                    &report["gating"],
                    &report["advisory"],
                )
                .map_err(bad_input)?;
                super::workspace::replace(&viewer_path, synced.as_bytes())?;
            }
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
            let mut report = json!({"flags":[flag], "gating":[flag], "advisory":[],
                "settled":false, "verdict":"FAILED", "refusal":refusal});
            if let Some(thumbs) = super::parse_thumb_pose(args)? {
                report["body"] = json!({"thumb_pose": thumbs});
            }
            if let Some(arms) = super::drape_pose(args)?.arm_pose {
                report["arm_pose"] = json!(arms);
                report["body"]["arm_pose"] = json!(arms);
            }
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

fn unused_qualification_path(ws: &Path, kind: &str) -> Result<PathBuf, Refusal> {
    for suffix in 0..u64::MAX {
        let path = ws.join(format!("qualification.{kind}-{suffix:03}"));
        match fs::symlink_metadata(&path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(path),
            Err(e) => return Err(io(&path, e)),
            Ok(_) => {}
        }
    }
    Err(bad_input("qualification directory numbers exhausted"))
}

/// With the workspace locked, finish the interrupted handoff on either side of publication.
fn recover_ws_qualification(ws: &Path) -> Result<(), Refusal> {
    let previous = ws.join("qualification.previous");
    if !regular(&previous, true)? {
        return Ok(());
    }
    let current = ws.join("qualification");
    let destination = if regular(&current, true)? {
        if !regular(&current.join("qualify.json"), false)? {
            return Err(
                bad_input("the active qualification is missing its receipt").repair(format!(
                    "The prior qualification is at {}",
                    previous.display()
                )),
            );
        }
        unused_qualification_path(ws, "before")?
    } else {
        current
    };
    fs::rename(&previous, &destination).map_err(|e| io(&destination, e))
}

fn publish_ws_qualification(ws: &Path, staged: &Path, current: &Path) -> Result<(), Refusal> {
    if regular(current, true)? {
        let previous = ws.join("qualification.previous");
        fs::rename(current, &previous).map_err(|e| io(&previous, e))?;
    }
    if let Err(e) = fs::rename(staged, current) {
        let mut refusal = io(current, e).repair(format!(
            "Check the staged qualification at {}",
            staged.display()
        ));
        if let Err(restore) = recover_ws_qualification(ws) {
            refusal = refusal.repair(format!(
                "Recover qualification.previous (restore failed: {})",
                restore.message
            ));
        }
        return Err(refusal);
    }
    recover_ws_qualification(ws).map_err(|refusal| {
        refusal
            .repair("The replacement is active. The previous qualification is still recoverable.")
    })
}

/// `ftx qualify`: run the three known-answer chest bands (`-20`, `0`, `+20` mm ease) at zero
/// gravity (`--sew 70 --drape 0`) and write `qualify.json` (`fabric-qualify/1`, spec 04 §7.1).
pub(super) fn cmd_qualify(args: &Args) -> Result<(Value, i32, String), Refusal> {
    use ftx_body::{BodyFrames, Pose, clothing_skin_with_hands, skin_mesh};
    use ftx_drape::animation::{mesh_map, pc2, seam_colours, strain_colours};
    use ftx_drape::qualify::{
        DEFAULT_BAND_HEIGHT_MM, QUALIFY_DRAPE_STEPS, QUALIFY_EASES, QUALIFY_SEW_STEPS,
        band_fit_spec, band_pattern_from_girth, evaluate_band, fabric_key,
    };
    use ftx_drape::run::{DrapeBody, drape, viewer};

    known_options(
        args,
        "qualify",
        &[
            "ws",
            "measurements",
            "pattern",
            "fabric",
            "material-bindings",
            "out",
            "model",
            "body-fit",
            "arm-deg",
            "left-arm-deg",
            "right-arm-deg",
            "thumb-pose",
            "legs",
            "leg-fraction",
            "skinning",
            "hands",
            "compression",
            "newton-strategy",
            "integrator",
            "rho-inf",
            "bending-model",
            "compression-model",
            "air-drag",
            "air-cn",
            "air-ct",
            "air-rho",
            "air-velocity",
            "rayleigh-membrane-s",
            "rayleigh-bending-s",
            "adaptive-barrier",
            "stick-slip-mus",
            "stick-slip-muk",
            "stick-slip-v",
            "construction-standard",
            "hem-width-mm",
            "hem-mass-mult",
            "hem-bending-mult",
            "seam-width-mm",
            "seam-mass-mult",
            "seam-bending-mult",
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
    let thumbs = super::parse_thumb_pose(args)?;
    let skinning = super::parse_skinning(args)?;
    let hands = super::parse_hands(args)?;
    let mut opts = super::drape_options(args)?;
    opts.sew_steps = QUALIFY_SEW_STEPS;
    opts.drape_steps = QUALIFY_DRAPE_STEPS;
    opts.validate()?;

    let input = super::fabric::load(args)?;
    let (data, meas_bytes, mut pat, published, _ws_lock) = if let Some(ws) = ws_arg.as_deref() {
        if args.options.contains_key("out") {
            return Err(bad_input(
                "--out cannot be combined with a workspace (workspace qualification writes to WS/qualification)",
            ));
        }
        let lock = super::workspace::Workspace::open(ws)?;
        recover_ws_qualification(ws)?;
        let meas_path = match args.options.get("measurements") {
            Some(m) => PathBuf::from(m),
            None => ws.join("measurements.json"),
        };
        let (raw_m, meas_bytes) = read_json(&meas_path.to_string_lossy())?;
        let data = measurements(&raw_m)?;
        let (_, prev_pat) = inventory(ws)?;
        let pat = match args.options.get("pattern") {
            Some(p) => super::fold::pattern_document(Path::new(p))?.0,
            None => match prev_pat {
                Some(p) => {
                    regular(&p, false)?;
                    super::fold::pattern_document(&p)?.0
                }
                None => json!({}),
            },
        };
        (data, meas_bytes, pat, ws.join("qualification"), Some(lock))
    } else {
        let meas_path = super::required(args, "measurements")?;
        let out = PathBuf::from(super::required(args, "out")?);
        let (raw_m, meas_bytes) = read_json(meas_path)?;
        let data = measurements(&raw_m)?;
        let pat = match args.options.get("pattern") {
            Some(p) => super::fold::pattern_document(Path::new(p))?.0,
            None => json!({}),
        };
        (data, meas_bytes, pat, out, None)
    };

    if !pat.is_object() && args.options.contains_key("fabric") {
        pat = json!({});
    }
    let fabric_override = input.map(|input| input.apply(&mut pat)).transpose()?;
    if fabric_override.is_some() {
        super::fabric::snapshot_bytes(&pat, "effective qualification pattern")?;
    }
    if ws_arg.is_none() {
        super::reserve_drape_output(&published)?;
    }

    let super::body_source::PreparedBody {
        model,
        params,
        description,
        ..
    } = super::body_source::prepare(args, Some(&data))?;
    let rest = model.rest(&params)?;
    let rest_skin = clothing_skin_with_hands(&model, &rest.vertices, hands)?;
    let posed = super::pose_fitted_body(&model, &rest, &pose, skinning, thumbs)?;
    let skin = clothing_skin_with_hands(&model, &posed.vertices, hands)?;
    let authored_hand_surface =
        super::audit_authored_hands(&model, &posed, &skin, hands, thumbs).transpose()?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let frames = BodyFrames::new(&model, &posed)?;
    let torso = frames.torso.origin / 1000.0;
    let chest_girth_mm = frames
        .landmark_h("torso", "chest")
        .and_then(|height_mm| {
            ftx_body::tape::girth(
                &skin.vertices,
                &skin.triangles,
                torso.z + height_mm / 1000.0,
                [torso.x, torso.y],
            )
        })
        .map(|girth_m| girth_m * 1000.0)
        .filter(|girth_mm| girth_mm.is_finite() && *girth_mm > 0.0)
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::DegenerateGeometry,
                "no closed torso section surrounding the axis at the posed chest landmark",
            )
        })?;
    let input_chest_mm = Measurements(data).value("chest").ok();
    let chest_girth_residual_mm = input_chest_mm.map(|target| chest_girth_mm - target);
    let wearer_target_aligned = chest_girth_residual_mm
        .is_some_and(|r| r.abs() <= ftx_drape::qualify::MAX_QUALIFIED_GIRTH_RESIDUAL_MM);
    let posed_frames = ftx_drape::scene::PosedFrames::new(&frames, pose.arm_pose);
    let body = DrapeBody {
        frames: &posed_frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
        arm_pose: pose.arm_pose,
    };
    let out = if let Some(ws) = ws_arg.as_deref() {
        regular(&published, true)?;
        let staged = unused_qualification_path(ws, "pending")?;
        super::reserve_output(&staged, "qualify")?;
        staged
    } else {
        published.clone()
    };
    let mut authored_pose = serde_json::Map::new();
    if let Some(arms) = pose.arm_pose {
        authored_pose.insert("arm_pose".into(), json!(arms));
    }
    if let Some(thumbs) = thumbs {
        authored_pose.insert("thumb_pose".into(), json!(thumbs));
    }
    if let Some(audit) = authored_hand_surface {
        authored_pose.insert(
            "authored_hand_surface_pairs_tested".into(),
            json!(audit.pairs_tested),
        );
    }
    // spec 04 §7.1: the measurements file's sha256 (what a round compares, whichever engine runs it)
    let meas_sha = ftx_core::receipt::sha256_hex(&meas_bytes);
    let spec = band_fit_spec();

    let run_three_bands = |group_out: &Path,
                           fabric_sel: Option<&Value>|
     -> Result<(Value, Vec<Value>, Vec<String>, bool), Refusal> {
        let mut group_basis = Value::Null;
        let mut bands = Vec::with_capacity(QUALIFY_EASES.len());
        for ease in QUALIFY_EASES {
            let bpat =
                band_pattern_from_girth(chest_girth_mm, ease, DEFAULT_BAND_HEIGHT_MM, fabric_sel)?;
            let material = ftx_drape::material::resolve(&Pattern::new(bpat.clone()), &opts)?;
            if bands.is_empty() {
                group_basis = ftx_drape::qualify::qualification_basis(&body, &material)?;
            }
            bands.push((ease, bpat));
        }
        let mut group_rows = Vec::with_capacity(bands.len());
        let mut group_lines = Vec::with_capacity(bands.len());
        for (ease, bpat) in bands {
            let bdir = group_out.join(format!("band_{ease:+}"));
            make_dir(&bdir)?;
            let pat_path = bdir.join("pattern.json");
            write_json(&pat_path, &bpat)?;
            write_json(&bdir.join("fit_spec.json"), &spec)?;
            let sim_dir = bdir.join("sim");
            super::reserve_drape_output(&sim_dir)?;
            let pattern = Pattern::new(bpat);
            let started = std::time::Instant::now();
            let mut run = drape(&pattern, &spec, &body, &opts, |line| eprintln!("{line}"))?;
            run.report["body_contact_proxy"] =
                skin.contact_proxy_basis_with_rest_and_skinning(&rest_skin, skinning);
            let mut report = run.report.clone();
            if !hands.is_wrist() {
                report["hand_collider"] = json!(hands.as_str());
            }
            if let Some(selected) = &fabric_override {
                report["fabric_override"] = selected.receipt().clone();
            }
            if let Some(o) = report.as_object_mut() {
                let relative = pat_path.strip_prefix(&out).map_err(|e| io(&pat_path, e))?;
                o.insert(
                    "pattern".into(),
                    json!(published.join(relative).to_string_lossy()),
                );
                o.insert("measurements_sha256".into(), json!(meas_sha.clone()));
                o.insert(
                    "wall_s".into(),
                    json!((started.elapsed().as_secs_f64() * 10.0).round() / 10.0),
                );
                let mut b = description.clone();
                b.insert(
                    "pose".into(),
                    json!(hands.drape_pose_line_with_arms(body.arm_deg, body.arm_pose)),
                );
                if !pose.legs.is_standing() {
                    b.insert("legs".into(), json!(pose.legs));
                }
                if pose.leg_fraction != 1.0 {
                    b.insert("leg_fraction".into(), json!(pose.leg_fraction));
                }
                if !skinning.is_lbs() {
                    b.insert("skinning".into(), json!(skinning.as_str()));
                }
                b.extend(authored_pose.clone());
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
                        super::obj(last, &mesh.faces, Some(&seam_colours(mesh, style)))
                            .into_bytes(),
                    ),
                    (
                        "garment_strain.obj",
                        super::obj(last, &mesh.faces, Some(&strain_colours(mesh, last)))
                            .into_bytes(),
                    ),
                    (
                        "body.obj",
                        super::obj(&full.vertices, &full.triangles, None).into_bytes(),
                    ),
                ],
            )?;
            let (row, line, _) = evaluate_band(ease, &report);
            group_rows.push(row);
            group_lines.push(line);
        }
        let group_pass = group_rows.iter().all(|r| r["pass"].as_bool() == Some(true));
        Ok((group_basis, group_rows, group_lines, group_pass))
    };

    let piece_selectors = ftx_drape::qualify::active_piece_fabric_selectors(&pat);
    let (qualification_basis, rows, lines, all_pass, piece_materials) = if piece_selectors
        .is_empty()
    {
        let (basis, rows, lines, pass) = run_three_bands(&out, pat.get("fabric"))?;
        (basis, rows, lines, pass, None)
    } else {
        let garment_mat = ftx_drape::material::resolve(&Pattern::new(pat.clone()), &opts)?;
        let basis = ftx_drape::qualify::qualification_basis(&body, &garment_mat)?;
        let mut groups: BTreeMap<String, (Value, Vec<String>)> = BTreeMap::new();
        for (pid, sel) in piece_selectors {
            let probe =
                band_pattern_from_girth(chest_girth_mm, 0, DEFAULT_BAND_HEIGHT_MM, Some(&sel))?;
            let umat = ftx_drape::material::resolve(&Pattern::new(probe), &opts)?;
            let ubasis = ftx_drape::qualify::qualification_basis(&body, &umat)?;
            let mat_sha = ubasis["material_sha256"]
                .as_str()
                .ok_or_else(|| bad_input("missing uniform material_sha256 for piece fabric"))?
                .to_string();
            groups
                .entry(mat_sha)
                .or_insert_with(|| (sel, Vec::new()))
                .1
                .push(pid);
        }
        let mut primary_rows = Vec::new();
        let mut all_lines = Vec::new();
        let mut all_groups_pass = true;
        let mut pm_records = Vec::with_capacity(groups.len());
        for (idx, (mat_sha, (sel, piece_ids))) in groups.into_iter().enumerate() {
            let group_dir = if idx == 0 {
                out.clone()
            } else {
                let d = out.join(format!("piece_material_{idx:02}"));
                make_dir(&d)?;
                d
            };
            let (_, group_rows, group_lines, group_pass) = run_three_bands(&group_dir, Some(&sel))?;
            if idx == 0 {
                primary_rows.clone_from(&group_rows);
            }
            all_lines.push(format!(
                "Piece material {} ({}):",
                &mat_sha[..12],
                piece_ids.join(", ")
            ));
            all_lines.extend(group_lines);
            all_groups_pass &= group_pass;
            pm_records.push(json!({
                "material_sha256": mat_sha,
                "fabric_key": fabric_key(Some(&sel)),
                "piece_ids": piece_ids,
                "rows": group_rows,
                "pass": group_pass,
            }));
        }
        (
            basis,
            primary_rows,
            all_lines,
            all_groups_pass,
            Some(pm_records),
        )
    };

    let mut rec = json!({
        "schema": "fabric-qualify/1",
        "engine": "ftx",
        "measurements_sha256": meas_sha,
        "posed_chest_girth_mm": chest_girth_mm,
        "input_chest_mm": input_chest_mm,
        "chest_girth_residual_mm": chest_girth_residual_mm,
        "max_wearer_girth_residual_mm": ftx_drape::qualify::MAX_QUALIFIED_GIRTH_RESIDUAL_MM,
        "wearer_target_aligned": wearer_target_aligned,
        "qualification_basis": qualification_basis,
        "fabric_key": fabric_key(pat.get("fabric")),
        "rows": rows,
        "collider_pass": all_pass,
        "pass": all_pass,
    });
    if let Some(pm) = piece_materials {
        rec["piece_materials"] = json!(pm);
    }
    if !hands.is_wrist() {
        rec["hand_collider"] = json!(hands.as_str());
    }
    if !authored_pose.is_empty() {
        rec["body"] = Value::Object(authored_pose);
    }
    if let Some(selected) = &fabric_override {
        selected.save(&out)?;
        rec["fabric_override"] = selected.receipt().clone();
    }
    write_json(&out.join("qualify.json"), &rec)?;
    if let Some(ws) = ws_arg.as_deref() {
        publish_ws_qualification(ws, &out, &published)?;
    }
    // spec 04 §7.1's printout: a line per band, then the verdict
    let mut text = format!("Posed torso girth at the chest landmark: {chest_girth_mm:.2} mm");
    if let Some(target) = input_chest_mm {
        text.push_str(&format!(" (input chest: {target:.2} mm)"));
    }
    text.push('\n');
    text.push_str(&lines.join("\n"));
    if all_pass && !wearer_target_aligned {
        match chest_girth_residual_mm {
            Some(res) => text.push_str(&format!(
                "\nNOTE: collider bands passed, but posed chest girth differs from input chest by {res:+.2} mm (max ±{:.1} mm); wearer tightness flags stay advisory",
                ftx_drape::qualify::MAX_QUALIFIED_GIRTH_RESIDUAL_MM
            )),
            None => text.push_str(
                "\nNOTE: collider bands passed, but chest target is absent from measurements; wearer tightness flags stay advisory",
            ),
        }
    }
    text.push_str(if all_pass {
        "\nQUALIFIED"
    } else {
        "\nNOT QUALIFIED: do not let the sim gate on tightness for this body/fabric"
    });
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
    fn a_measurement_file_without_readings_is_refused_with_the_way_to_make_one() {
        // What `ftx drape` accepts as explicit targets: a schema label and a fused block, no readings.
        let fused_only = json!({"schema": "fabric-measurements/1",
            "wearer": {"label": "Synthetic", "stature_mm": 1780},
            "fused": {"chest": {"value_mm": 1000}}});
        let refusal = measurements(&fused_only).unwrap_err();
        assert!(
            refusal.message.contains("records must be a list"),
            "{}",
            refusal.message
        );
        assert_eq!(refusal.repairs.len(), 1, "{:?}", refusal.repairs);
        for needed in [
            "needs a `records` list",
            "ftx measure init",
            "ftx measure add",
            "ftx measure reference",
            "`ftx drape` and `ftx place` accept explicit targets without records",
        ] {
            assert!(
                refusal.repairs[0].contains(needed),
                "{needed}: {}",
                refusal.repairs[0]
            );
        }
        // The same for a `records` key that is present but not a list.
        let mut null_records = fused_only.clone();
        null_records["records"] = Value::Null;
        assert_eq!(measurements(&null_records).unwrap_err().repairs.len(), 1);

        // A file that has readings and fails for another reason is told only what is wrong with it.
        let bad_reading = json!({"schema": "fabric-measurements/1", "wearer": {"label": "Synthetic"},
            "records": [{"name": "chest", "value_mm": 1000, "sigma_mm": 5, "provenance": "guesswork"}]});
        let other = measurements(&bad_reading).unwrap_err();
        assert!(other.message.contains("provenance"), "{}", other.message);
        assert!(other.repairs.is_empty(), "{:?}", other.repairs);

        // Legacy flat measurements, with neither key, still pass through untouched.
        let flat = json!({"chest": 1000, "waist": 860});
        assert_eq!(measurements(&flat).unwrap(), flat);
    }

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

    #[test]
    fn round_compression_requires_sim_and_validates_before_workspace_io() {
        let missing_sim =
            super::super::parse(&["round", "no-such-ws", "--compression", "0.2"].map(String::from))
                .unwrap();
        let err = cmd_round(&missing_sim).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("--compression requires --sim"));

        for bad in ["0", "-0.1", "1.5", "NaN", "soft"] {
            let args = super::super::parse(
                &["round", "no-such-ws", "--sim", "--compression", bad].map(String::from),
            )
            .unwrap();
            let err = cmd_round(&args).unwrap_err();
            assert_ne!(
                err.code,
                RefusalCode::Io,
                "invalid --compression {bad} must refuse before reading workspace"
            );
        }
    }

    #[test]
    fn qualify_accepts_physics_options_and_validates_before_workspace_io() {
        // Valid physics/constitutive options must pass option preflight and reach workspace I/O.
        let valid = super::super::parse(
            &[
                "qualify",
                "--measurements",
                "no-such-measurements.json",
                "--out",
                "no-such-out",
                "--compression",
                "0.25",
                "--bending-model",
                "discrete-shells",
                "--compression-model",
                "principal",
                "--air-drag",
                "--air-cn",
                "1.1",
                "--air-velocity",
                "-2.5,1.25,0",
                "--rayleigh-membrane-s",
                "0.002",
                "--rayleigh-bending-s",
                "0.003",
                "--adaptive-barrier",
                "--stick-slip-mus",
                "0.6",
                "--stick-slip-muk",
                "0.3",
                "--stick-slip-v",
                "0.02",
                "--construction-standard",
                "--newton-strategy",
                "adaptive-ilu",
                "--integrator",
                "generalized-alpha",
                "--rho-inf",
                "0.5",
            ]
            .map(String::from),
        )
        .unwrap();
        let err = cmd_qualify(&valid).unwrap_err();
        assert_eq!(
            err.code,
            RefusalCode::Io,
            "valid qualify physics options must pass option parsing/validation before reading files, got: {err}"
        );

        // Invalid physics options must refuse fail-closed BEFORE any workspace/file I/O.
        for bad_args in [
            vec!["qualify", "no-such-ws", "--compression", "0"],
            vec!["qualify", "no-such-ws", "--bending-model", "bogus"],
            vec!["qualify", "no-such-ws", "--compression-model", "bogus"],
            vec!["qualify", "no-such-ws", "--air-cn", "1.0"],
            vec!["qualify", "no-such-ws", "--air-velocity", "1,0,0"],
            vec![
                "qualify",
                "no-such-ws",
                "--air-drag",
                "--air-velocity",
                "1,2",
            ],
            vec![
                "qualify",
                "no-such-ws",
                "--air-drag",
                "--air-velocity",
                "NaN,0,0",
            ],
            vec!["qualify", "no-such-ws", "--rayleigh-membrane-s", "-0.1"],
            vec![
                "qualify",
                "no-such-ws",
                "--stick-slip-mus",
                "0.2",
                "--stick-slip-muk",
                "0.5",
            ],
            vec!["qualify", "no-such-ws", "--hem-mass-mult", "2.0"],
            vec!["qualify", "no-such-ws", "--rho-inf", "0.5"],
        ] {
            let argv: Vec<String> = bad_args.into_iter().map(String::from).collect();
            let parsed = super::super::parse(&argv).unwrap();
            let err = cmd_qualify(&parsed).unwrap_err();
            assert_ne!(
                err.code,
                RefusalCode::Io,
                "invalid qualify option {argv:?} must refuse before reading workspace"
            );
        }

        // Schedule and self-contact flags remain rejected on `ftx qualify` (spec 04 §7.1 fixes
        // the three body-only zero-gravity chest bands at 70 sewing / 0 gravity steps).
        for disallowed in [
            vec!["qualify", "no-such-ws", "--sew", "12"],
            vec!["qualify", "no-such-ws", "--drape", "10"],
            vec!["qualify", "no-such-ws", "--self-contact-mm", "2"],
            vec![
                "qualify",
                "no-such-ws",
                "--self-stick-slip-mus",
                "0.5",
                "--self-stick-slip-muk",
                "0.3",
            ],
        ] {
            let argv: Vec<String> = disallowed.into_iter().map(String::from).collect();
            let parsed = super::super::parse(&argv).unwrap();
            let err = cmd_qualify(&parsed).unwrap_err();
            assert_eq!(
                err.code,
                RefusalCode::InvalidDocument,
                "disallowed qualify flag {argv:?} must be refused as unknown"
            );
            assert!(
                err.message.contains("has no option"),
                "expected unknown-option refusal for {argv:?}, got: {err}"
            );
        }
    }

    #[test]
    fn round_sim_text_surfaces_opt_in_diagnostics_without_changing_defaults() {
        let mut default_text = String::new();
        super::append_round_sim_text(
            &mut default_text,
            &json!({
                "gating": [],
                "advisory": [],
                "membrane_history": {"integration": "backward_euler"},
                "elastic_hold_window": {"requested_steps": 0, "status": "not_requested"}
            }),
        );
        assert_eq!(
            default_text,
            "\nSIM gating flags: none\nSIM advisory flags: none"
        );

        let mut opt_in_text = String::new();
        super::append_round_sim_text(
            &mut opt_in_text,
            &json!({
                "gating": [],
                "advisory": ["one"],
                "numerical": {
                    "time_subdivision": {
                        "subdivided_nominal_intervals": 2,
                        "nominal_intervals": 8,
                        "accepted_substeps": 12,
                        "trials": 15
                    }
                },
                "start_repair": {
                    "basis": "opposing_leg_placement",
                    "moved_welded_vertices": 6,
                    "maximum_displacement_mm": 1.5
                },
                "elastic_hold_window": {"requested_steps": 4, "status": "available"}
            }),
        );
        assert!(opt_in_text.contains("\nSIM advisory flags: one"));
        assert!(
            opt_in_text.contains(
                "\nSIM time subdivision: 2/8 intervals subdivided (12 substeps, 15 trials)"
            )
        );
        assert!(
            opt_in_text
                .contains("\nSIM start repair: opposing leg separation (6 vertices, max 1.5 mm)")
        );
        assert!(opt_in_text.contains("\nSIM elastic hold window: 4 step(s) (available)"));

        let mut casing_text = String::new();
        super::append_round_sim_text(
            &mut casing_text,
            &json!({
                "gating": [],
                "advisory": [],
                "elastic_hold_window": {"requested_steps": 4, "status": "available"},
                "elastic_observation_windows": [{
                    "instance": "waistband",
                    "declared_hold": {
                        "status": "available",
                        "signed_vertical_displacement_m": -0.0004,
                        "maximum_absolute_vertical_displacement_m": 0.0005,
                        "maximum_vertex_speed_m_s": 0.0008,
                        "quiet_at_1_mm_s": true
                    }
                }]
            }),
        );
        assert!(casing_text.contains(
            "\nSIM elastic hold window: 4 step(s) (available) — waistband: Δz -0.4 mm, max |Δz| 0.5 mm, max speed 0.8 mm/s, quiet ≤1 mm/s"
        ));
    }
}
