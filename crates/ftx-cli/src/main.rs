//! `ftx`: the FrankenTextiles command line (plan §14). It mirrors the reference's `fab` commands so the
//! skill can call it subsystem by subsystem as each reaches parity. This first cut has:
//!
//! - `ftx validate PATTERN --measurements M [--fit F] [--json]`: the flat validation (seam walk, truing,
//!   points of measure), bit-exact with the reference.
//! - `ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--elastic-ea N]
//!   [--model PATH] [--self-contact-mm MM] [--self-mu MU] [--json]`: an Anny body fitted to the wearer's
//!   measurements in the drape pose, the pattern meshed,
//!   placed, sewn and draped on it, and `sim_report.json`, `fold.pc2`, `mesh_map.json`,
//!   `fold_viewer.html` and the last frame as OBJ written to DIR.
//! - `ftx body --measurements M --out DIR [--pose drape|rest|arms-up|seated|squat] [--model PATH]
//!   [--json]`: that body alone, as `body.obj` and `body.json` (`ftx-body/1`).
//!
//! Every command prints human text, or with `--json` one JSON envelope `{"schema": "ftx-cli/1",
//! "command", "ok", "exit", "result" | "refusal"}`. Exit codes: 0 converged / no gating flags, 2 not
//! converged / gating flags, 3 refused input or (validate) BLOCKED, 4 budget exceeded. A refusal's text goes to stderr, a
//! result's to stdout.
#![forbid(unsafe_code)]

mod body_source;
mod draft;
mod export;
mod fold;
mod measure;
mod outfit;
mod render;
mod round;
mod trace;
mod two_start;
mod workspace;

use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

use ftx_body::{
    AnnyModel, BodyFrames, FREE, Pose, TAPE_FIELDS, TapeFit, Wearer, default_model_path, fit_tape,
    pose_deltas, skin_mesh, tape_targets, trimmed_skin,
};
use ftx_core::Vec3;
use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_drape::animation::{mesh_map, pc2, seam_colours, strain_colours};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape, viewer};
use ftx_pattern::{Measurements, Pattern, alter, format_table, validate};
use serde_json::{Value, json};

const VIEWER_TEMPLATE: &str = include_str!("../../../assets/fold_viewer_template.html");

const USAGE: &str = "usage:
  ftx draft zouave_bolero|zouave_trousers|fez --measurements M --out DIR [--style S.json] [--json]
  ftx draft PROGRAM --measurements M --ws WS [--style S.json] [--update-measurements] [--as-of DATE] [--no-priors] [--json]
  ftx validate PATTERN --measurements M [--fit F] [--json]
  ftx round WS [--pattern P] [--measurements M] [--fit F] [--export]
            [--sim|--stills] [--body-fit REPORT] [--arm-deg DEG] [--res WIDTHxHEIGHT]
            [--update-measurements] [--note TEXT] [--as-of DATE] [--created YYYY-MM-DDTHH:MM] [--json]
  ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--elastic-ea N]
            [--two-start-sew N] [--spread-vertex-mm 10] [--spread-hem-mm 6]
            [--spread-clearance-mm 3] [--spread-seam-pp 1.5]
            [--model PATH] [--body-fit REPORT] [--arm-deg DEG] [--self-contact-mm MM] [--self-mu MU] [--json]
  ftx body --measurements M --out DIR [--pose drape|rest|arms-up|seated|squat] [--model PATH] [--json]
  ftx body --body-fit REPORT --out NEW_DIR [--measurements M] [--pose rest|drape|arms-up|seated|squat] [--model PATH] [--json]
  ftx render DRAPE_DIR [--res 960x540] [--json]
  ftx sim-points SIMDIR --pattern P --measurements M [--json]
  ftx outfit --measurements M --sim DIR [--sim DIR ...] --out NEW_DIR [--hat P] [--seams] [--res 960x960] [--json]
  ftx alter slash-spread PATTERN --piece P --axis x|y --at MM --add MM --out OUT [--json]
  ftx alter move PATTERN --piece P --vertex V [--dx MM] [--dy MM] --out OUT [--json]
  ftx alter true-seam PATTERN --seam S [--end start|end] --out OUT [--json]
  ftx alter cap-ease PATTERN --piece P --seams S1,S2 --target RATIO --out OUT [--json]
  ftx alter transfer-curve PATTERN --from SRC.json:INSTANCE/EDGE --to PIECE/EDGE --out OUT [--json]
  ftx measure catalogue [--garment KIND] [--out FILE] [--json]
  ftx measure prior|reference --stature-mm MM [--age-months N] [--sex M|F] [--out FILE] [--json]
  ftx measure init --label NICKNAME --out FILE [--stature 92cm] [--age-months N] [--taken-on DATE] [--json]
  ftx measure add FILE --name NAME --value 55cm [--value 55.5cm] [--sigma 1cm] [--provenance SOURCE] [--out FILE] [--json]
  ftx measure validate FILE [--json]
  ftx measure photo-scale --image PNG [--paper letter|a4|auto] [--out FILE] [--json]
  ftx measure photo-scale --reference JSON [--out FILE] [--json]
  ftx measure measure-photo --image PNG|--reference JSON --points JSON [--paper letter|a4|auto]
      [--camera-distance-mm MM] [--depth-offset-mm MM] [--click-px PX] [--bootstrap N] [--seed N]
      [--taken-on DATE] [--out FILE] [--json]
  ftx measure fuse FILES... [--no-priors] [--out FILE] [--json]
  ftx measure ready FILES... --garment KIND [--as-of DATE] [--no-priors] [--closed-cuff] [--passes-hip]
      [--nappies|--no-nappies] [--elastic-waist] [--cuff-level knee|calf|ankle] [--cuff-opening] [--out FILE] [--json]
  ftx export TARGET PATTERN --out DIR [--unfold] [--sa MM] [--mitre-limit N] [--projector-width MM]
            [--layout-width MM] [--keep-blank-pages] [--hpgl-rotate90] [--hpgl-mirror] [--created YYYY-MM-DDTHH:MM] [--json]
      TARGET: svg pdf letter a4 copyshop projector dxf astm aama flat-dxf hpgl cutter garmentcode embroidery all
  ftx check DIR [--pattern-ref PATTERN] [--json]
  ftx trace IMAGE.png|.jpg --out DIR [--crop X,Y,W,H] [--channel auto|bgdist|dark|light|gray|sat|r|g|b|edge]
            [--invert|--no-invert] [--bg R,G,B] [--stroke PX] [--close 3] [--open PX] [--corner-deg 35]
            [--soft-deg 70] [--simplify-frac 0.003] [--smooth 1.2] [--min-area-frac 0.01]
            [--erase X,Y,W,H ...] [--no-dashes] [--px-out FILE] [--json]
  ftx annotate DRAFT --names NAMES.json --out PATTERN [--json]
  ftx scale-fit PATTERN --target REF=LEN [--target REF=LEN ...] --out PATTERN [--json]
  ftx audit PATTERN --out DIR [--labels L.json] [--tol-mm 3] [--median-seam-mm 250] [--label-tol 0.03] [--json]
  ftx --version";

/// Parsed command line: the positional arguments and the `--flag value` options.
struct Args {
    positional: Vec<String>,
    /// `--flag value` options; a repeated `--target` (scale-fit) or `--erase` (trace) keeps every value,
    /// newline-joined.
    options: BTreeMap<String, String>,
    values: Vec<String>,
    json: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let (mut positional, mut options, mut values, mut json) =
        (Vec::new(), BTreeMap::new(), Vec::new(), false);
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            json = true;
        } else if let Some(name) = a.strip_prefix("--") {
            // The reference uses --sim both as a round switch and a repeated outfit path;
            // --seams is a switch here, but takes a value in alter cap-ease.
            if argv.first().is_some_and(|s| s == "outfit") && name == "sim" {
                let value = it.next().ok_or("--sim needs a directory")?;
                if value.starts_with("--") {
                    return Err("--sim needs a directory".into());
                }
                values.push(value.clone());
                options.insert(name.to_string(), value.clone());
                continue;
            }
            if measure::BOOLEAN_OPTIONS.contains(&name)
                || export::BOOLEAN_OPTIONS.contains(&name)
                || round::BOOLEAN_OPTIONS.contains(&name)
                || trace::BOOLEAN_OPTIONS.contains(&name)
                || (name == "seams" && argv.first().is_some_and(|s| s == "outfit"))
            {
                if options
                    .insert(name.to_string(), "true".to_string())
                    .is_some()
                {
                    return Err(format!("--{name} was supplied more than once"));
                }
                continue;
            }
            let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
            if value.starts_with("--") {
                return Err(format!("--{name} needs a value, got {value}"));
            }
            if name == "value" {
                values.push(value.clone());
            }
            if name == "target" || name == "erase" {
                options
                    .entry(name.to_string())
                    .and_modify(|v: &mut String| {
                        v.push('\n');
                        v.push_str(value);
                    })
                    .or_insert_with(|| value.clone());
                continue;
            }
            if options.insert(name.to_string(), value.clone()).is_some() && name != "value" {
                return Err(format!("--{name} was supplied more than once"));
            }
        } else {
            positional.push(a.clone());
        }
    }
    Ok(Args {
        positional,
        options,
        values,
        json,
    })
}

/// A Wavefront OBJ (metres; `v x y z [r g b]`, 1-based faces).
fn obj(vertices: &[Vec3], faces: &[[usize; 3]], colours: Option<&[[f64; 3]]>) -> String {
    use std::fmt::Write as _;
    let mut s = String::new();
    for (i, v) in vertices.iter().enumerate() {
        let _ = write!(s, "v {} {} {}", v.x, v.y, v.z);
        if let Some(c) = colours.and_then(|c| c.get(i)) {
            let _ = write!(s, " {} {} {}", c[0], c[1], c[2]);
        }
        s.push('\n');
    }
    for f in faces {
        let _ = writeln!(s, "f {} {} {}", f[0] + 1, f[1] + 1, f[2] + 1);
    }
    s
}

fn bad_input(m: impl Into<String>) -> Refusal {
    Refusal::new(RefusalCode::InvalidDocument, m)
}

fn read_json(path: &str) -> Result<(Value, Vec<u8>), Refusal> {
    let bytes =
        std::fs::read(path).map_err(|e| Refusal::new(RefusalCode::Io, format!("{path}: {e}")))?;
    let v = serde_json::from_slice(&bytes).map_err(|e| bad_input(format!("{path}: {e}")))?;
    Ok((v, bytes))
}

/// Refuses an option the command does not take (a misspelt `--fit` must not silently drop the fit spec).
fn known_options(args: &Args, command: &str, allowed: &[&str]) -> Result<(), Refusal> {
    match args.options.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(bad_input(format!("ftx {command} has no option --{k}"))
            .repair(format!("options: --{}", allowed.join(", --")))),
        None => Ok(()),
    }
}

fn required<'a>(args: &'a Args, name: &str) -> Result<&'a str, Refusal> {
    args.options
        .get(name)
        .map(String::as_str)
        .ok_or_else(|| bad_input(format!("--{name} is required")))
}

fn fit_spec(args: &Args) -> Result<Value, Refusal> {
    match args.options.get("fit") {
        Some(p) => Ok(read_json(p)?.0),
        None => Ok(json!({"points": []})),
    }
}

fn make_dir(dir: &std::path::Path) -> Result<(), Refusal> {
    std::fs::create_dir_all(dir)
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", dir.display())))
}

/// The Anny model (`--model`, else the converted default).
fn load_model(args: &Args) -> Result<AnnyModel, Refusal> {
    let path = args
        .options
        .get("model")
        .map(PathBuf::from)
        .or_else(default_model_path)
        .ok_or_else(|| {
            Refusal::new(RefusalCode::Io, "no body model path")
                .repair("convert Anny with tools/body-model/convert_anny.py, or pass --model")
        })?;
    AnnyModel::load(&path, false)
}

/// The wearer of a measurement file and the body fitted to it on the drape pose (a line on stderr).
fn fitted_body(model: &AnnyModel, meas: &Value) -> Result<(Wearer, TapeFit), Refusal> {
    let w = Wearer::from_measurements(meas)?;
    let fit = fit_tape(model, &w, &tape_targets(meas), &Pose::drape())?;
    eprintln!(
        "body: fitted in {} iterations; {}; {:.2} kg{}",
        fit.iterations,
        fit.residuals
            .iter()
            .map(|r| format!(
                "{} {:.1} ({:+.1})",
                r.name,
                r.body_mm,
                r.body_mm - r.target_mm
            ))
            .collect::<Vec<_>>()
            .join(", "),
        fit.mass_kg,
        w.weight_kg
            .map_or(String::new(), |kg| format!(" (weighs {kg:.2})"))
    );
    Ok((w, fit))
}

/// The fitted body as `drape` reports it and `body` writes it.
fn body_json(w: &Wearer, fit: &TapeFit) -> serde_json::Map<String, Value> {
    let mm = |v: f64| (v * 10.0).round() / 10.0;
    let kg = |v: f64| (v * 100.0).round() / 100.0;
    let named =
        |pairs: &mut dyn Iterator<Item = (String, Value)>| pairs.collect::<serde_json::Map<_, _>>();
    let mut m = serde_json::Map::new();
    m.insert("model".into(), json!("anny 0.6.1"));
    m.insert("age_years".into(), json!(w.years));
    m.insert("gender".into(), json!(w.gender));
    m.insert(
        "fit".into(),
        json!({
            "iterations": fit.iterations,
            "tape": named(&mut fit.residuals.iter().map(|r| (r.name.clone(), json!({
                "target_mm": r.target_mm,
                "body_mm": mm(r.body_mm),
            })))),
            "free": named(&mut FREE.iter().zip(fit.free).map(|(n, v)| {
                ((*n).to_string(), json!((v * 1e4).round() / 1e4))
            })),
            "weight_kg": w.weight_kg.map(|t| json!({"target_kg": t, "body_kg": kg(fit.mass_kg)})),
        }),
    );
    m.insert(
        "readings_mm".into(),
        Value::Object(named(
            &mut TAPE_FIELDS
                .iter()
                .zip(fit.readings)
                .map(|(n, v)| ((*n).to_string(), json!(mm(v)))),
        )),
    );
    m.insert("mass_kg".into(), json!(kg(fit.mass_kg)));
    m.insert("model_fit".into(), Value::Array(model_fit_records(w, fit)));
    m
}

/// The body's readings of every [`TAPE_FIELDS`] measurement the fit was not given, as `fabric-measurements/1`
/// records with provenance `MODEL_FIT` (plan §9.4: read off the fitted body, never passed off as measured)
/// and the population spread as σ: the chart prior's σ at the wearer's stature and age, or the catalogue's
/// `MODEL_FIT` default (12 mm) outside the charts.
fn model_fit_records(w: &Wearer, fit: &TapeFit) -> Vec<Value> {
    let sex = if w.gender == 0.0 {
        Some("M")
    } else if w.gender == 1.0 {
        Some("F")
    } else {
        None
    };
    let priors = ftx_measure::priors_for(w.stature_mm, Some(w.years * 12.0), sex).ok();
    let method = format!(
        "ftx body: Anny 0.6.1 fitted to {} tape measurements",
        fit.residuals.len()
    );
    TAPE_FIELDS
        .iter()
        .zip(fit.readings)
        .filter(|(n, _)| !fit.residuals.iter().any(|r| r.name == **n))
        .filter_map(|(n, v)| {
            let chart = priors.as_ref().and_then(|p| p[*n]["sigma_mm"].as_f64());
            let (sigma, note) = match chart {
                Some(s) => (
                    s,
                    "σ = the population spread at this stature and age (chart prior)",
                ),
                None => (
                    12.0,
                    "σ = the catalogue's MODEL_FIT default (no chart prior here)",
                ),
            };
            ftx_measure::make_record(
                n,
                (v * 10.0).round() / 10.0,
                sigma,
                "MODEL_FIT",
                &method,
                None,
                note,
            )
            .ok()
        })
        .collect()
}

/// `ftx body`: the body fitted to a measurement file, posed, as `body.obj` and `body.json`
/// (`ftx-body/1`: the fit, the readings, the phenotype and the local changes in use).
fn cmd_body(args: &Args) -> Result<(Value, i32, String), Refusal> {
    if args.options.contains_key("body-fit") {
        return body_source::cmd_body(args);
    }
    known_options(args, "body", &["measurements", "out", "pose", "model"])?;
    let (meas, meas_bytes) = read_json(required(args, "measurements")?)?;
    let out = PathBuf::from(required(args, "out")?);
    let pose_name = args.options.get("pose").map_or("drape", String::as_str);
    let pose = match pose_name {
        "drape" => Pose::drape(),
        "rest" => Pose::rest(),
        "arms-up" => Pose::arms_up(),
        "seated" => Pose::seated(),
        "squat" => Pose::squat(),
        other => {
            return Err(bad_input(format!("--pose {other}: no such pose"))
                .repair("--pose drape, rest, arms-up, seated or squat"));
        }
    };
    make_dir(&out)?;
    let model = load_model(args)?;
    let (w, fit) = fitted_body(&model, &meas)?;
    let rest = model.rest(&fit.params)?;
    let posed = model.pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?)?;
    let skin = skin_mesh(&posed.vertices, &model.data().faces)?;
    let ph = fit.params.phenotype;
    let labels = &model.data().header.labels.local_changes;
    let mut doc = body_json(&w, &fit);
    doc.insert("schema".into(), json!("ftx-body/1"));
    doc.insert("measurements_sha256".into(), json!(sha256_hex(&meas_bytes)));
    doc.insert("pose".into(), json!(pose_name));
    doc.insert(
        "phenotype".into(),
        json!({"gender": ph.gender, "age": ph.age, "height": ph.height, "weight": ph.weight,
               "muscle": ph.muscle, "proportions": ph.proportions}),
    );
    doc.insert(
        "local_changes".into(),
        Value::Object(
            labels
                .iter()
                .zip(&fit.params.local_changes)
                .filter(|(_, v)| **v != 0.0)
                .map(|(n, v)| (n.clone(), json!(v)))
                .collect(),
        ),
    );
    let write = |name: &str, bytes: &[u8]| -> Result<(), Refusal> {
        std::fs::write(out.join(name), bytes)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{name}: {e}")))
    };
    write(
        "body.obj",
        obj(&skin.vertices, &skin.triangles, None).as_bytes(),
    )?;
    let doc = Value::Object(doc);
    write(
        "body.json",
        serde_json::to_string_pretty(&doc)
            .unwrap_or_default()
            .as_bytes(),
    )?;
    let text = format!(
        "{} mm, {:.2} kg, {} tape measurements fitted (largest residual {:.1} mm); pose {pose_name}\nwrote {}",
        (fit.readings[0] * 10.0).round() / 10.0,
        fit.mass_kg,
        fit.residuals.len(),
        fit.residuals
            .iter()
            .map(|r| (r.body_mm - r.target_mm).abs())
            .fold(0.0, f64::max),
        out.display()
    );
    Ok((
        json!({"out": out, "files": ["body.obj", "body.json"], "body": doc}),
        0,
        text,
    ))
}

/// `ftx alter OP PATTERN --out OUT [...]`: one alteration op (spec 01 §10.7), the result written even
/// with structure errors (exit 2 then), the op's history sentence printed.
fn cmd_alter(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let op = args.positional.get(1).map_or("", String::as_str);
    let allowed: &[&str] = match op {
        "slash-spread" => &["out", "piece", "axis", "at", "add"],
        "move" => &["out", "piece", "vertex", "dx", "dy"],
        "true-seam" => &["out", "seam", "end"],
        "cap-ease" => &["out", "piece", "seams", "target"],
        "transfer-curve" => &["out", "from", "to"],
        _ => {
            return Err(bad_input(format!("ftx alter: no op {op:?}")).repair(
                "ftx alter slash-spread|move|true-seam|cap-ease|transfer-curve PATTERN ...",
            ));
        }
    };
    known_options(args, "alter", allowed)?;
    if args.positional.len() != 3 {
        return Err(bad_input(format!(
            "ftx alter {op} needs exactly one pattern file"
        )));
    }
    let pattern_path = args
        .positional
        .get(2)
        .ok_or_else(|| bad_input(format!("ftx alter {op} needs a pattern file")))?;
    let (doc, _) = read_json(pattern_path)?;
    let out = PathBuf::from(required(args, "out")?);
    let text = |k: &str| required(args, k);
    let num = |k: &str, default: Option<f64>| -> Result<f64, Refusal> {
        match (args.options.get(k), default) {
            (Some(v), _) => v
                .parse()
                .map_err(|_| bad_input(format!("--{k} must be a number"))),
            (None, Some(d)) => Ok(d),
            (None, None) => Err(bad_input(format!("--{k} is required"))),
        }
    };
    let result = match op {
        "slash-spread" => {
            let axis = text("axis")?;
            if axis != "x" && axis != "y" {
                return Err(bad_input("--axis must be x or y"));
            }
            alter::slash_spread(
                &doc,
                text("piece")?,
                axis,
                num("at", None)?,
                num("add", None)?,
            )
        }
        "move" => alter::move_vertex(
            &doc,
            text("piece")?,
            text("vertex")?,
            num("dx", Some(0.0))?,
            num("dy", Some(0.0))?,
        ),
        "true-seam" => {
            let end = args.options.get("end").map_or("end", String::as_str);
            if end != "start" && end != "end" {
                return Err(bad_input("--end must be start or end"));
            }
            alter::true_seam(&doc, text("seam")?, end)
        }
        "cap-ease" => {
            let seams: Vec<String> = text("seams")?.split(',').map(String::from).collect();
            alter::cap_ease(&doc, text("piece")?, &seams, num("target", None)?)
        }
        _ => {
            let from = text("from")?;
            let (src_path, src_ref) = from
                .rsplit_once(':')
                .ok_or_else(|| bad_input("--from must be SRC.json:instance/edge"))?;
            let (src, _) = read_json(src_path)?;
            alter::transfer_curve(&doc, &src, src_ref, text("to")?)
        }
    }?;
    let errors = Pattern::new(result.clone()).structure_errors();
    // Finish serialization before touching the filesystem. create_new is an atomic no-clobber
    // check, including symlinks/hard links: a mistaken --out must not destroy the source pattern.
    let body = serde_json::to_string_pretty(&result)
        .map_err(|e| bad_input(format!("cannot serialize altered pattern: {e}")))?
        + "\n";
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .map_err(|e| {
            Refusal::new(RefusalCode::Io, format!("{}: {e}", out.display()))
                .repair("choose a new --out path; existing pattern files are never overwritten")
        })?;
    file.write_all(body.as_bytes())
        .and_then(|()| file.sync_all())
        .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", out.display())))?;
    let why = result["meta"]["history"]
        .as_array()
        .and_then(|h| h.last())
        .and_then(|h| h["why"].as_str())
        .unwrap_or_default()
        .to_string();
    let mut text_out = why.clone();
    if !errors.is_empty() {
        text_out.push_str("\nstructure errors:");
        for e in &errors {
            text_out.push_str(&format!("\n  {e}"));
        }
    }
    let code = if errors.is_empty() { 0 } else { 2 };
    Ok((
        json!({"out": out, "why": why, "structure_errors": errors}),
        code,
        text_out,
    ))
}

fn cmd_validate(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "validate", &["measurements", "fit"])?;
    let pattern_path = args
        .positional
        .get(1)
        .ok_or_else(|| bad_input("ftx validate needs a pattern file"))?;
    let (pat, _) = read_json(pattern_path)?;
    let (meas, _) = read_json(required(args, "measurements")?)?;
    let spec = fit_spec(args)?;
    let rep = validate(&Pattern::new(pat), &spec, &Measurements(meas));
    let result = serde_json::to_value(&rep).unwrap_or(Value::Null);
    Ok((result, rep.exit_code(), format_table(&rep)))
}

/// Validate settings before reading files, fitting the body or creating outputs.
fn drape_options(args: &Args) -> Result<DrapeOptions, Refusal> {
    let mut opts = DrapeOptions::default();
    for (name, slot) in [
        ("sew", &mut opts.sew_steps),
        ("drape", &mut opts.drape_steps),
    ] {
        if let Some(v) = args.options.get(name) {
            *slot = v
                .parse()
                .map_err(|_| bad_input(format!("--{name} must be a whole number")))?;
        }
    }
    if let Some(mm) = args.options.get("self-contact-mm") {
        let mut p = opts.contact;
        p.thickness = mm
            .parse::<f64>()
            .map_err(|_| bad_input("--self-contact-mm must be a distance in millimetres"))?
            * 1e-3;
        if let Some(mu) = args.options.get("self-mu") {
            p.mu = mu
                .parse()
                .map_err(|_| bad_input("--self-mu must be a non-negative number"))?;
        }
        opts.self_contact = Some(p);
    } else if args.options.contains_key("self-mu") {
        return Err(bad_input("--self-mu requires --self-contact-mm")
            .repair("specify the sum of the cloth layers' half-thicknesses in millimetres"));
    }
    if let Some(v) = args.options.get("elastic-ea") {
        opts.elastic_ea = v
            .parse()
            .map_err(|_| bad_input("--elastic-ea must be a number of newtons (0: no elastic)"))?;
    }
    opts.validate()?;
    Ok(opts)
}

/// Pose the same fitted body at the requested angle; do not change its tape-fit reference pose.
fn drape_pose(args: &Args) -> Result<Pose, Refusal> {
    let mut pose = Pose::drape();
    if let Some(value) = args.options.get("arm-deg") {
        let angle = value
            .parse::<f64>()
            .map_err(|_| bad_input("--arm-deg must be a finite angle in degrees"))?;
        Refusal::finite("--arm-deg", angle)?;
        if !(0.0..=180.0).contains(&angle) {
            return Err(Refusal::new(
                RefusalCode::OutOfRange,
                "--arm-deg must be in 0..=180 degrees from vertical",
            ));
        }
        pose.arm_abduction_deg = Some(angle);
    }
    Ok(pose)
}

/// Claim a fresh destination before fitting/solving. A failed retry must not leave a previous
/// successful run at the path the caller will inspect for its new result.
fn reserve_drape_output(out: &std::path::Path) -> Result<(), Refusal> {
    match std::fs::symlink_metadata(out) {
        Ok(_) => {
            return Err(bad_input(format!(
                "{}: drape output already exists; use a new directory for every run",
                out.display()
            ))
            .repair("choose a fresh --out path; completed and failed runs are never reused"));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(workspace::io_error(out, e)),
    }
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        make_dir(parent)?;
    }
    std::fs::create_dir(out).map_err(|e| workspace::io_error(out, e))
}

/// All serialization succeeds before publication starts. The report is the final completion
/// record, never an invitation to consume stale or half-written geometry from an earlier run.
fn write_drape_output(
    out: &std::path::Path,
    report: &Value,
    files: &[(&str, Vec<u8>)],
) -> Result<(), Refusal> {
    let bytes = serde_json::to_vec_pretty(report).map_err(|e| bad_input(e.to_string()))?;
    for (name, bytes) in files {
        workspace::write_new(&out.join(name), bytes)?;
    }
    workspace::write_new(&out.join("sim_report.json"), &bytes)
}

fn cmd_drape(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "drape",
        &[
            "measurements",
            "out",
            "fit",
            "sew",
            "drape",
            "model",
            "self-contact-mm",
            "self-mu",
            "elastic-ea",
            "arm-deg",
            "body-fit",
            "two-start-sew",
            "spread-vertex-mm",
            "spread-hem-mm",
            "spread-clearance-mm",
            "spread-seam-pp",
        ],
    )?;
    let opts = drape_options(args)?;
    let pair = two_start::options(args, &opts)?;
    let pose = drape_pose(args)?;
    let pattern_path = args
        .positional
        .get(1)
        .ok_or_else(|| bad_input("ftx drape needs a pattern file"))?;
    let (pat, _) = read_json(pattern_path)?;
    let meas_path = required(args, "measurements")?;
    let (meas, meas_bytes) = read_json(meas_path)?;
    let out = PathBuf::from(required(args, "out")?);
    let spec = fit_spec(args)?;
    // Refuse unwritable/existing destinations before the expensive fit and drape.
    reserve_drape_output(&out)?;
    let body_source::PreparedBody {
        model,
        params,
        description,
        ..
    } = body_source::prepare(args, Some(&meas))?;
    let rest = model.rest(&params)?;
    let posed = model.pose(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?)?;
    let skin = trimmed_skin(
        &model,
        &posed.vertices,
        &["wrist.R", "wrist.L", "foot.R", "foot.L"],
    )?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let frames = BodyFrames::new(&model, &posed)?;
    let pattern = Pattern::new(pat);
    let body = DrapeBody {
        frames: &frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
    };
    let started = std::time::Instant::now();
    let run = match pair {
        Some(policy) => {
            ftx_drape::two_start::drape(&pattern, &spec, &body, &opts, &policy, |line| {
                eprintln!("{line}")
            })?
        }
        None => drape(&pattern, &spec, &body, &opts, |line| eprintln!("{line}"))?,
    };
    let mut report = run.report.clone();
    if let Some(o) = report.as_object_mut() {
        o.insert("pattern".into(), json!(pattern_path));
        o.insert("measurements_sha256".into(), json!(sha256_hex(&meas_bytes)));
        o.insert(
            "wall_s".into(),
            json!((started.elapsed().as_secs_f64() * 10.0).round() / 10.0),
        );
        let mut b = description;
        b.insert(
            "pose".into(),
            json!(format!(
                "drape: arms at {}° and straight, hands and feet left out of contact",
                body.arm_deg
            )),
        );
        o.insert("body".into(), Value::Object(b));
    }
    let html = viewer(&run, &pattern, &body, VIEWER_TEMPLATE).map_err(bad_input)?;
    // the last frame and the body as OBJ, for renders outside (the skill's Blender stills): the
    // garment once with its seam colours and once with its strain colours (`v x y z r g b`)
    let mesh = &run.scene.mesh;
    let last = run.fold.frames.last().map_or(&[][..], Vec::as_slice);
    let style = pattern.raw()["style"].as_str().unwrap_or("");
    write_drape_output(
        &out,
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
                obj(last, &mesh.faces, Some(&seam_colours(mesh, style))).into_bytes(),
            ),
            (
                "garment_strain.obj",
                obj(last, &mesh.faces, Some(&strain_colours(mesh, last))).into_bytes(),
            ),
            (
                "body.obj",
                obj(&full.vertices, &full.triangles, None).into_bytes(),
            ),
        ],
    )?;
    let (gating, advisory) = (run.gating(), run.advisory());
    let mut text = format!(
        "fit: {} gating, {} advisory, {}\n",
        gating.len(),
        advisory.len(),
        if report["settled"].as_bool() == Some(true) {
            "settled"
        } else {
            "not settled"
        }
    );
    for f in &gating {
        text.push_str(&format!("  GATING   {f}\n"));
    }
    for f in &advisory {
        text.push_str(&format!("  advisory {f}\n"));
    }
    text.push_str(if opts.self_contact.is_some() {
        "self-contact: enabled; solver-state audits passed\n"
    } else {
        "self-contact: not simulated\n"
    });
    text.push_str(&format!("wrote {}", out.display()));
    let pair_summary = two_start::summary(&report);
    if !pair_summary.is_empty() {
        text.push_str(&format!("\n{pair_summary}"));
    }
    let code = if gating.is_empty() { 0 } else { 2 };
    let mut result = json!({
        "out": out,
        "gating": gating,
        "advisory": advisory,
        "settled": report["settled"],
        "self_contact": report["self_contact"],
        "files": ["sim_report.json", "fold.pc2", "mesh_map.json", "fold_viewer.html",
                  "garment.obj", "garment_strain.obj", "body.obj"],
    });
    if let Some(pair) = report.get("two_start") {
        result["two_start"] = pair.clone();
    }
    Ok((result, code, text))
}

fn emit(command: &str, json_out: bool, outcome: Result<(Value, i32, String), Refusal>) -> ExitCode {
    // a refusal's text goes to stderr; a result's to stdout whatever its exit code (a BLOCKED
    // validation exits 3 with a table)
    let (envelope, code, text, refused) = match outcome {
        Ok((result, code, text)) => (
            json!({"schema": "ftx-cli/1", "command": command, "ok": code == 0, "exit": code, "result": result}),
            code,
            text,
            false,
        ),
        Err(r) => {
            let code = if r.code == RefusalCode::BudgetExceeded {
                4
            } else {
                3
            };
            (
                json!({"schema": "ftx-cli/1", "command": command, "ok": false, "exit": code, "refusal": r}),
                code,
                format!("{r}"),
                true,
            )
        }
    };
    if json_out {
        println!("{envelope}");
    } else if refused {
        eprintln!("{text}");
    } else if !text.is_empty() {
        println!("{text}");
    }
    ExitCode::from(u8::try_from(code).unwrap_or(3))
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    if argv.iter().any(|a| a == "--version") {
        println!("ftx {}", env!("CARGO_PKG_VERSION"));
        return ExitCode::SUCCESS;
    }
    if argv.is_empty() || argv[0] == "help" || argv.iter().any(|a| a == "--help") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }
    let args = match parse(&argv) {
        Ok(a) => a,
        Err(e) => {
            return emit(
                argv.first().map_or("ftx", String::as_str),
                argv.iter().any(|a| a == "--json"),
                Err(bad_input(e).repair(USAGE)),
            );
        }
    };
    match args.positional.first().map(String::as_str) {
        Some("draft") => emit("draft", args.json, draft::cmd_draft(&args)),
        Some("render") => emit("render", args.json, render::cmd_render(&args)),
        Some("sim-points") => emit("sim-points", args.json, fold::sim_points(&args)),
        Some("outfit") => emit("outfit", args.json, outfit::cmd_outfit(&args)),
        Some("validate") => emit("validate", args.json, cmd_validate(&args)),
        Some("round") => emit("round", args.json, round::cmd_round(&args)),
        Some("drape") => emit("drape", args.json, cmd_drape(&args)),
        Some("body") => emit("body", args.json, cmd_body(&args)),
        Some("alter") => emit("alter", args.json, cmd_alter(&args)),
        Some("measure") => emit("measure", args.json, measure::run(&args)),
        Some("export") => emit("export", args.json, export::cmd_export(&args)),
        Some("trace") => emit("trace", args.json, trace::cmd_trace(&args)),
        Some("annotate") => emit("annotate", args.json, trace::cmd_annotate(&args)),
        Some("scale-fit") => emit("scale-fit", args.json, trace::cmd_scale_fit(&args)),
        Some("audit") => emit("audit", args.json, trace::cmd_audit(&args)),
        Some("check") => emit(
            "check",
            args.json,
            export::cmd_check(&args, args.positional.get(1)),
        ),
        other => emit(
            other.unwrap_or("ftx"),
            args.json,
            Err(bad_input("unknown or missing command").repair(USAGE)),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn drape_scratch() -> PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let time = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let path = std::env::temp_dir().join(format!(
            "ftx-drape-output-{}-{time}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        path
    }

    #[test]
    fn drape_refuses_existing_outputs_before_loading_the_body() {
        let dir = drape_scratch();
        let out = dir.join("run");
        std::fs::create_dir(&out).unwrap();
        let previous = br#"{"settled":true,"gating":[],"previous_run":true}"#;
        std::fs::write(out.join("sim_report.json"), previous).unwrap();
        std::fs::write(dir.join("pattern.json"), b"{}").unwrap();
        std::fs::write(dir.join("measurements.json"), b"{}").unwrap();
        let args = parse(&[
            "drape".into(),
            dir.join("pattern.json").to_string_lossy().into_owned(),
            "--measurements".into(),
            dir.join("measurements.json").to_string_lossy().into_owned(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
            "--model".into(),
            dir.join("absent.ftxbody").to_string_lossy().into_owned(),
        ])
        .unwrap();
        let error = cmd_drape(&args).unwrap_err();
        assert!(error.message.contains("already exists"), "{error}");
        assert_eq!(
            std::fs::read(out.join("sim_report.json")).unwrap(),
            previous
        );
        assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
        let mut fresh = args;
        fresh.options.insert(
            "out".into(),
            dir.join("fresh").to_string_lossy().into_owned(),
        );
        assert_eq!(cmd_drape(&fresh).unwrap_err().code, RefusalCode::Io);
        assert!(dir.join("fresh").is_dir());
        assert!(!dir.join("fresh/sim_report.json").exists());
        assert!(
            cmd_drape(&fresh)
                .unwrap_err()
                .message
                .contains("already exists")
        );
    }

    #[test]
    fn drape_partial_geometry_never_gets_a_completion_report() {
        let files = [
            ("fold.pc2", b"cache".to_vec()),
            ("mesh_map.json", b"{}".to_vec()),
            ("fold_viewer.html", b"viewer".to_vec()),
            ("garment.obj", b"cloth".to_vec()),
            ("garment_strain.obj", b"strain".to_vec()),
            ("body.obj", b"body".to_vec()),
        ];
        for fail_at in 0..files.len() {
            let out = drape_scratch().join("run");
            reserve_drape_output(&out).unwrap();
            // A deterministic filesystem failure at each artifact; no solver or fault harness.
            std::fs::create_dir(out.join(files[fail_at].0)).unwrap();
            assert!(write_drape_output(&out, &json!({"settled":true}), &files).is_err());
            assert!(!out.join("sim_report.json").exists());
            for (name, bytes) in &files[..fail_at] {
                assert_eq!(std::fs::read(out.join(name)).unwrap(), *bytes);
            }
            assert!(out.join(files[fail_at].0).is_dir());
            assert!(reserve_drape_output(&out).is_err());
        }
    }

    #[test]
    fn drape_publication_is_complete_and_existing_evidence_is_not_overwritten() {
        let out = drape_scratch().join("nested/run");
        reserve_drape_output(&out).unwrap();
        let report = json!({"settled":true,"gating":[]});
        let files = [("fold.pc2", b"synthetic cache".to_vec())];
        write_drape_output(&out, &report, &files).unwrap();
        let bytes = std::fs::read(out.join("sim_report.json")).unwrap();
        assert_eq!(serde_json::from_slice::<Value>(&bytes).unwrap(), report);
        assert_eq!(std::fs::read(out.join("fold.pc2")).unwrap(), files[0].1);
        assert!(write_drape_output(&out, &json!({"settled":false}), &files).is_err());
        assert_eq!(std::fs::read(out.join("sim_report.json")).unwrap(), bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{PermissionsExt, symlink};
            for name in ["sim_report.json", "fold.pc2"] {
                assert_eq!(
                    std::fs::metadata(out.join(name))
                        .unwrap()
                        .permissions()
                        .mode()
                        & 0o777,
                    0o600
                );
            }
            let alias = out.parent().unwrap().join("alias");
            symlink(&out, &alias).unwrap();
            assert!(reserve_drape_output(&alias).is_err());
            let dangling = out.parent().unwrap().join("dangling");
            symlink("no-such-target", &dangling).unwrap();
            assert!(reserve_drape_output(&dangling).is_err());
            assert!(!out.parent().unwrap().join("no-such-target").exists());
        }
    }

    #[test]
    fn explicit_arm_angles_including_zero_reach_the_straight_arm_pose() {
        let default = drape_pose(&parse(&["drape".into()]).unwrap()).unwrap();
        assert_eq!(default, Pose::drape());
        for angle in [0.0, 35.0, 50.0, 150.0, 180.0] {
            let args = parse(&["drape".into(), "--arm-deg".into(), angle.to_string()]).unwrap();
            let pose = drape_pose(&args).unwrap();
            assert_eq!(pose.arm_abduction_deg, Some(angle));
            assert!(pose.straight_arms);
            assert_eq!(pose.legs, default.legs);
        }
        for angle in ["NaN", "inf", "-0.1", "180.1", "arms-up"] {
            let args =
                parse(&["drape", "absent.json", "--arm-deg", angle].map(String::from)).unwrap();
            assert_ne!(cmd_drape(&args).unwrap_err().code, RefusalCode::Io);
        }
    }

    #[test]
    fn drape_self_contact_units_and_friction_are_independent() {
        let args =
            parse(&["drape", "--self-contact-mm", "0.6", "--self-mu", "0.2"].map(String::from))
                .unwrap();
        let opts = drape_options(&args).unwrap();
        let self_params = opts.self_contact.unwrap();
        assert!((self_params.thickness - 0.0006).abs() < 1e-15);
        assert_eq!(self_params.mu, 0.2);
        assert_eq!(opts.contact, DrapeOptions::default().contact);
        let default = drape_options(&parse(&["drape".into()]).unwrap()).unwrap();
        assert!(default.self_contact.is_none());
    }

    #[test]
    fn bad_drape_settings_refuse_before_file_io() {
        for options in [
            vec!["--self-contact-mm", "NaN"],
            vec!["--self-contact-mm", "inf"],
            vec!["--self-contact-mm", "-1"],
            vec!["--self-contact-mm", "cloth"],
            vec!["--self-contact-mm", "1", "--self-mu", "-0.1"],
            vec!["--self-contact-mm", "1", "--self-mu", "NaN"],
            vec!["--self-mu", "0.4"],
            vec!["--sew", "0", "--drape", "0"],
        ] {
            let mut argv = vec!["drape", "file-that-does-not-exist.json"];
            argv.extend(options);
            let args = parse(&argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            let err = cmd_drape(&args).unwrap_err();
            assert_ne!(err.code, RefusalCode::Io, "{}", err.message);
        }
        let args = parse(&["drape".into(), "--sew".into(), usize::MAX.to_string()]).unwrap();
        assert_eq!(
            drape_options(&args).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
    }

    #[test]
    fn options_and_flags_parse() {
        let a = parse(&["drape", "p.json", "--out", "d", "--json", "--sew", "3"].map(String::from))
            .unwrap();
        assert_eq!(a.positional, ["drape", "p.json"]);
        assert_eq!(a.options["out"], "d");
        assert_eq!(a.options["sew"], "3");
        assert!(a.json);
        assert!(parse(&["drape".to_string(), "--out".to_string()]).is_err());
    }

    #[test]
    fn obj_with_and_without_colours() {
        let v = [Vec3::new(0.0, 0.5, 1.0), Vec3::new(1.0, 0.0, 0.0)];
        let f = [[0, 1, 0]];
        assert_eq!(obj(&v, &f, None), "v 0 0.5 1\nv 1 0 0\nf 1 2 1\n");
        let c = [[1.0, 0.0, 0.25], [0.5, 0.5, 0.5]];
        assert_eq!(
            obj(&v, &f, Some(&c)),
            "v 0 0.5 1 1 0 0.25\nv 1 0 0 0.5 0.5 0.5\nf 1 2 1\n"
        );
    }

    #[test]
    fn a_misspelt_option_is_refused() {
        let a = parse(&["validate", "p.json", "--measurment", "m.json"].map(String::from)).unwrap();
        let r = known_options(&a, "validate", &["measurements", "fit"]).unwrap_err();
        assert_eq!(r.code, RefusalCode::InvalidDocument);
        assert!(r.message.contains("--measurment"), "{}", r.message);
        let ok = parse(&["validate", "p.json", "--fit", "f.json"].map(String::from)).unwrap();
        assert!(known_options(&ok, "validate", &["measurements", "fit"]).is_ok());
    }
}
