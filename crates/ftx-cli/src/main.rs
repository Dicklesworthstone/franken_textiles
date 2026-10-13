//! `ftx`: the FrankenTextiles command line (plan §14). It mirrors the reference's `fab` commands so the
//! skill can call it subsystem by subsystem as each reaches parity. This first cut has:
//!
//! - `ftx validate PATTERN --measurements M [--fit F] [--json]`: the flat validation (seam walk, truing,
//!   points of measure), bit-exact with the reference.
//! - `ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--model PATH]
//!   [--json]`: an Anny body of the wearer's age, sex and stature in the drape pose, the pattern meshed,
//!   placed, sewn and draped on it, and `sim_report.json`, `fold.pc2`, `mesh_map.json` and
//!   `fold_viewer.html` written to DIR.
//!
//! Every command prints human text, or with `--json` one JSON envelope `{"schema": "ftx-cli/1",
//! "command", "ok", "exit", "result" | "refusal"}`. Exit codes: 0 converged / no gating flags, 2 not
//! converged / gating flags, 3 refused input.
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use ftx_body::{
    AnnyModel, BodyFrames, Pose, Wearer, default_model_path, fit_stature, pose_deltas, skin_mesh,
    trimmed_skin,
};
use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_drape::animation::{mesh_map, pc2};
use ftx_drape::run::{DrapeBody, DrapeOptions, drape, viewer};
use ftx_pattern::{Measurements, Pattern, format_table, validate};
use serde_json::{Value, json};

const VIEWER_TEMPLATE: &str = include_str!("../../../assets/fold_viewer_template.html");

const USAGE: &str = "usage:
  ftx validate PATTERN --measurements M [--fit F] [--json]
  ftx drape PATTERN --measurements M --out DIR [--fit F] [--sew N] [--drape N] [--model PATH] [--json]
  ftx --version";

/// Parsed command line: the positional arguments and the `--flag value` options.
struct Args {
    positional: Vec<String>,
    options: BTreeMap<String, String>,
    json: bool,
}

fn parse(argv: &[String]) -> Result<Args, String> {
    let (mut positional, mut options, mut json) = (Vec::new(), BTreeMap::new(), false);
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            json = true;
        } else if let Some(name) = a.strip_prefix("--") {
            let value = it.next().ok_or_else(|| format!("--{name} needs a value"))?;
            options.insert(name.to_string(), value.clone());
        } else {
            positional.push(a.clone());
        }
    }
    Ok(Args {
        positional,
        options,
        json,
    })
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

/// The wearer: age from `wearer.age_months`, stature from `wearer.stature_mm` or the fused stature, sex
/// from `wearer.sex`.
fn wearer(meas: &Value) -> Result<Wearer, Refusal> {
    let w = &meas["wearer"];
    let months = w["age_months"].as_f64().ok_or_else(|| {
        bad_input("the measurement file has no wearer.age_months")
            .repair("add the wearer's age in months")
    })?;
    let stature = w["stature_mm"]
        .as_f64()
        .or_else(|| meas["fused"]["stature"]["value_mm"].as_f64())
        .ok_or_else(|| {
            bad_input("the measurement file has no stature (wearer.stature_mm or a fused stature)")
                .repair("measure the stature, or fuse a stature prior")
        })?;
    Ok(Wearer {
        years: months / 12.0,
        gender: Wearer::gender_of(w["sex"].as_str()),
        stature_mm: stature,
    })
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

fn cmd_drape(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "drape",
        &["measurements", "out", "fit", "sew", "drape", "model"],
    )?;
    let pattern_path = args
        .positional
        .get(1)
        .ok_or_else(|| bad_input("ftx drape needs a pattern file"))?;
    let (pat, _) = read_json(pattern_path)?;
    let meas_path = required(args, "measurements")?;
    let (meas, meas_bytes) = read_json(meas_path)?;
    let out = PathBuf::from(required(args, "out")?);
    let spec = fit_spec(args)?;
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
    let model_path = args
        .options
        .get("model")
        .map(PathBuf::from)
        .or_else(default_model_path)
        .ok_or_else(|| {
            Refusal::new(RefusalCode::Io, "no body model path")
                .repair("convert Anny with tools/body-model/convert_anny.py, or pass --model")
        })?;
    let io = |e: std::io::Error| Refusal::new(RefusalCode::Io, format!("{}: {e}", out.display()));
    // before the minutes of draping, so an unwritable DIR refuses at once
    std::fs::create_dir_all(&out).map_err(io)?;
    let model = AnnyModel::load(&model_path, false)?;
    let w = wearer(&meas)?;
    let (params, stature) = fit_stature(&model, &w)?;
    let rest = model.rest(&params)?;
    let pose = Pose::drape();
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
    let run = drape(&pattern, &spec, &body, &opts, |line| eprintln!("{line}"))?;
    let mut report = run.report.clone();
    if let Some(o) = report.as_object_mut() {
        o.insert("pattern".into(), json!(pattern_path));
        o.insert("measurements_sha256".into(), json!(sha256_hex(&meas_bytes)));
        o.insert(
            "wall_s".into(),
            json!((started.elapsed().as_secs_f64() * 10.0).round() / 10.0),
        );
        o.insert(
            "body".into(),
            json!({
                "model": "anny 0.6.1",
                "age_years": w.years,
                "gender": w.gender,
                "stature_mm": (stature * 10.0).round() / 10.0,
                "pose": format!(
                    "drape: arms at {}° and straight, hands and feet left out of contact",
                    body.arm_deg
                ),
            }),
        );
    }
    let write = |name: &str, bytes: &[u8]| -> Result<(), Refusal> {
        std::fs::write(out.join(name), bytes)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{name}: {e}")))
    };
    write(
        "sim_report.json",
        serde_json::to_string_pretty(&report)
            .unwrap_or_default()
            .as_bytes(),
    )?;
    write("fold.pc2", &pc2(&run.fold.frames))?;
    write(
        "mesh_map.json",
        serde_json::to_string_pretty(&mesh_map(&run.scene.mesh, &run.fold))
            .unwrap_or_default()
            .as_bytes(),
    )?;
    let html = viewer(&run, &pattern, &body, VIEWER_TEMPLATE).map_err(bad_input)?;
    write("fold_viewer.html", html.as_bytes())?;
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
    text.push_str(&format!("wrote {}", out.display()));
    let code = if gating.is_empty() { 0 } else { 2 };
    Ok((
        json!({
            "out": out,
            "gating": gating,
            "advisory": advisory,
            "settled": report["settled"],
            "files": ["sim_report.json", "fold.pc2", "mesh_map.json", "fold_viewer.html"],
        }),
        code,
        text,
    ))
}

fn emit(command: &str, json_out: bool, outcome: Result<(Value, i32, String), Refusal>) -> ExitCode {
    let (envelope, code, text) = match outcome {
        Ok((result, code, text)) => (
            json!({"schema": "ftx-cli/1", "command": command, "ok": code == 0, "exit": code, "result": result}),
            code,
            text,
        ),
        Err(r) => (
            json!({"schema": "ftx-cli/1", "command": command, "ok": false, "exit": 3, "refusal": r}),
            3,
            format!("{r}"),
        ),
    };
    if json_out {
        println!("{envelope}");
    } else if code == 3 {
        eprintln!("{text}");
    } else {
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
            eprintln!("{e}\n{USAGE}");
            return ExitCode::from(3);
        }
    };
    match args.positional.first().map(String::as_str) {
        Some("validate") => emit("validate", args.json, cmd_validate(&args)),
        Some("drape") => emit("drape", args.json, cmd_drape(&args)),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(3)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_misspelt_option_is_refused() {
        let a = parse(&["validate", "p.json", "--measurment", "m.json"].map(String::from)).unwrap();
        let r = known_options(&a, "validate", &["measurements", "fit"]).unwrap_err();
        assert_eq!(r.code, RefusalCode::InvalidDocument);
        assert!(r.message.contains("--measurment"), "{}", r.message);
        let ok = parse(&["validate", "p.json", "--fit", "f.json"].map(String::from)).unwrap();
        assert!(known_options(&ok, "validate", &["measurements", "fit"]).is_ok());
    }

    #[test]
    fn wearer_from_a_measurement_file() {
        let m = json!({"wearer": {"age_months": 30.0, "stature_mm": 920.0, "sex": "girl"}});
        let w = wearer(&m).unwrap();
        assert_eq!((w.years, w.gender, w.stature_mm), (2.5, 1.0, 920.0));
        let fused =
            json!({"wearer": {"age_months": 24.0}, "fused": {"stature": {"value_mm": 870.0}}});
        assert_eq!(wearer(&fused).unwrap().stature_mm, 870.0);
        assert!(wearer(&json!({"wearer": {}})).is_err());
    }
}
