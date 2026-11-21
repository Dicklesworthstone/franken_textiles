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
mod fabric;
mod fold;
mod import;
mod inspect_pixel;
mod materials;
mod measure;
mod outfit;
mod place;
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
    AnnyModel, BodyFrames, FREE, HandCollider, Legs, Pose, SkinningMode, TAPE_FIELDS, TapeFit,
    Wearer, clothing_skin_with_hands, default_model_path, fit_tape, pose_deltas, skin_mesh,
    tape_targets,
};
use ftx_core::Vec3;
use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_drape::animation::{mesh_map, pc2, seam_colours, strain_colours};
use ftx_drape::run::{DrapeBody, DrapeOptions, StaticEquilibriumOptions, drape_observed, viewer};
use ftx_drape::stepping::SubdivisionOptions;
use ftx_drape::{NewtonStrategy, TimeIntegrator};
use ftx_pattern::{Measurements, Pattern, alter, format_table, validate};
use serde_json::{Value, json};

const VIEWER_TEMPLATE: &str = include_str!("../../../assets/fold_viewer_template.html");

const USAGE: &str = "usage:
  ftx import garmentcode INPUT.json --out NEW_DIR [--annotations ANNOTATIONS.json] [--json]
  ftx draft zouave_bolero|zouave_trousers|fez --measurements M --out DIR [--style S.json] [--fabric F.json] [--json]
  ftx draft PROGRAM --measurements M --ws WS [--style S.json] [--fabric F.json] [--update-measurements] [--as-of DATE] [--no-priors] [--json]
            [--model PATH] [--body-fit REPORT] (draft from a tape-fitted or accepted body; MODEL_FIT, not new measurements)
  ftx validate PATTERN --measurements M [--fit F] [--model PATH] [--body-fit REPORT] [--flat-body] [--json]
  ftx round WS [--pattern P] [--measurements M] [--fit F] [--fabric F.json] [--export]
            [--sim|--stills] [--model PATH] [--body-fit REPORT] [--flat-body] [--spacing-mm MM] [--compression F] [--arm-deg DEG] [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm] [--max-settle-steps N] [--res WIDTHxHEIGHT]
            [--static-equilibrium] [--static-tol-mm 0.001] [--static-max-newton 300]
            [--time-step-s SECONDS] [--sew N] [--drape N] [--time-subdivision] [--subdivision-max-halvings 8] [--subdivision-max-trials 4096]
            [--newton-strategy projected|adaptive-ilu|trust-region] [--newton-tol-m-s M/S] [--integrator implicit-euler|generalized-alpha] [--rho-inf 0.0]
            [--update-measurements] [--note TEXT] [--as-of DATE] [--created YYYY-MM-DDTHH:MM] [--json]
  ftx drape PATTERN --measurements M --out DIR [--fit F] [--fabric F.json] [--sew N] [--drape N] [--time-step-s SECONDS] [--elastic-ea N] [--elastic-hold-steps N]
            [--two-start-sew N] [--spread-vertex-mm 10] [--spread-hem-mm 6]
            [--spread-clearance-mm 3] [--spread-seam-pp 1.5]
            [--model PATH] [--body-fit REPORT] [--arm-deg DEG] [--spacing-mm MM] [--compression F] [--self-contact-mm MM] [--self-mu MU] [--json]
            [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm] [--max-settle-steps N]
            [--static-equilibrium] [--static-tol-mm 0.001] [--static-max-newton 300] (independent frictionless comparison)
            [--time-subdivision] [--subdivision-max-halvings 8] [--subdivision-max-trials 4096]
            [--newton-strategy projected|adaptive-ilu|trust-region] [--newton-tol-m-s M/S] [--integrator implicit-euler|generalized-alpha] [--rho-inf 0.0]
  ftx drape OUTFIT.json --measurements M --out NEW_DIR --self-contact-mm MM [--model PATH] [--json]
  ftx qualify (--ws WS | --measurements M --out DIR) [--pattern P] [--fabric F.json] [--model PATH] [--body-fit REPORT] [--arm-deg DEG]
              [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--json]
  ftx body --measurements M --out DIR [--pose drape|rest|arms-up|seated|squat] [--skinning lbs|dqs] [--model PATH] [--json]
  ftx body --body-fit REPORT --out NEW_DIR [--measurements M] [--pose rest|drape|arms-up|seated|squat] [--skinning lbs|dqs] [--model PATH] [--json]
  ftx place PATTERN --measurements M --out NEW_DIR [--model PATH] [--body-fit REPORT] [--spacing-mm MM] [--arm-deg DEG]
            [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm] [--res 960x540] [--json] (start-state stills and numbers; no drape)
  ftx render DRAPE_DIR [--res 960x540] [--json]
  ftx render DRAPE_DIR --compare REVISED_DIR --view VIEW --out NEW_DIR [--res 960x540] [--json]
  ftx inspect-pixel ROUND_DIR --view front|side|back|three_quarter|strain_front|strain_back --pixel X,Y [--json]
  ftx viewer SIMDIR [--out HTML] [--pattern P] [--json]
  ftx viewer SIMDIR --check-contact [--json]
  ftx report WS [--json]
  ftx sim-points SIMDIR --pattern P --measurements M [--json]
  ftx outfit --measurements M --sim DIR [--sim DIR ...] --out NEW_DIR [--hat P] [--seams] [--res 960x960] [--json]
  ftx alter slash-spread PATTERN --piece P --axis x|y --at MM --add MM --out OUT [--json]
  ftx alter move PATTERN --piece P --vertex V [--dx MM] [--dy MM] --out OUT [--json]
  ftx alter true-seam PATTERN --seam S [--end start|end] --out OUT [--json]
  ftx alter cap-ease PATTERN --piece P --seams S1,S2 --target RATIO --out OUT [--json]
  ftx alter transfer-curve PATTERN --from SRC.json:INSTANCE/EDGE --to PIECE/EDGE --out OUT [--json]
  ftx materials [list|catalog [SEARCH]|show ID|freeze ID|compare ID ID] [--allow-estimates] [--json]
  ftx materials inspect SELECTION.json [--json]
  ftx materials resolve PATTERN.json [--json]
  ftx materials bind PATTERN.json SELECTION.json NEW-PATTERN.json [--json]
  ftx materials price PATTERN.json QUOTES.json SCOPE.json [--json]
  ftx materials calibrate SWATCHES.json [--allow-estimates] [--json]
  ftx materials linearize-origin REQUEST.json [--json]
  ftx materials calibrate-bending COUPONS.json [--allow-estimates] [--json]
  ftx materials calibrate-friction SLIDING.json [--allow-estimates] [--json]
  ftx materials strip-acquisition ACQUISITION.json [--json]
  ftx materials strip-experiment REQUEST.json [--json]
  ftx materials calibrate-strip REQUEST.json [--allow-estimates] [--json]
  ftx materials cylinder-experiment REQUEST.json [--json]
  ftx materials calibrate-cylinder REQUEST.json [--allow-estimates] [--json]
  ftx materials treatment REQUEST.json [--json]
  ftx materials treatment-pattern PATTERN.json REQUEST.json [NEW-PATTERN.json] [--json]
  ftx materials thermal|thermal-transient STACK.json [--json]
  ftx materials thermal-at ID TEMPERATURE-C [--allow-estimates] [--json]
  ftx materials thermal-surface SIM_REPORT.json CONDITIONS.json [--json]
  ftx materials thermal-coverage SIM_REPORT.json COVERAGE.json CONDITIONS.json [--json]
  ftx materials thermal-map SIM_REPORT.json BODY.obj PROJECTION.json [--json]
  ftx materials thermal-motion MOTION.json [--json]
  ftx materials microclimate REQUEST.json [--json]
  ftx materials lower-visco REQUEST.json [--json]
  ftx materials sew-plan|sew-stitches PATTERN.json OPTIONS.json [--json]
  ftx materials sew-select PATTERN.json REQUEST.json [--json]
  ftx materials sew-svg PATTERN.json OPTIONS.json OPERATION-ID NEW.svg [--json]
  ftx materials sew-cell-svg PATTERN.json OPTIONS.json CELL-ID OPERATION-ID NEW.svg [--json]
  ftx measure catalogue [--garment KIND] [--out FILE] [--json]
  ftx measure prior|reference --stature-mm MM [--age-months N] [--sex M|F] [--out FILE] [--json]
  ftx measure init --label NICKNAME --out FILE [--stature 92cm] [--age-months N] [--weight-kg KG] [--bmi KG_PER_M2] [--taken-on DATE] [--json]
  ftx measure add FILE --name NAME --value 55cm [--value 55.5cm] [--sigma 1cm] [--provenance SOURCE] [--out FILE] [--json]
  ftx measure validate FILE [--json]
  ftx measure calib-sheet --paper letter|a4 --out DIR [--squares-x N] [--squares-y N] [--square-mm MM] [--marker-mm MM] [--json]
  ftx measure calibrate-camera FILE [--out FILE] [--json]
  ftx measure photo-scale --image PNG [--paper letter|a4|auto] [--sheet JSON] [--out FILE] [--json]
  ftx measure photo-scale --reference JSON [--out FILE] [--json]
  ftx measure sheet-pose --image PNG --sheet JSON [--out FILE] [--json]
  ftx measure sheet-pose --reference JSON [--calibration JSON] [--out FILE] [--json]
  ftx measure measure-photo --image PNG|--reference JSON --points JSON [--paper letter|a4|auto] [--sheet JSON]
      [--camera-distance-mm MM] [--depth-offset-mm MM] [--click-px PX] [--bootstrap N] [--seed N]
      [--taken-on DATE] [--out FILE] [--json]
  ftx measure girth-from-section (--from FILE | --width-mm W --depth-mm D) [--level waist|chest|hip]
      [--sigma-width-mm SW] [--sigma-depth-mm SD] [--k K] [--k-sigma SK] [--k-ratio R] [--k-ratio-sigma SR]
      [--clothing-mm C] [--tape-girth-mm T] [--sigma-tape-mm ST] [--name NAME] [--out FILE] [--json]
  ftx measure triangulate FILE [--click-noise JSON] [--lens-noise JSON] [--sheet-noise JSON]
      [--reference-scale-noise JSON] [--out FILE] [--json]
  ftx measure triangulate-distance FILE [--out FILE] [--json]
  ftx measure bundle-adjust FILE [--iterations N] [--out FILE] [--json]
  ftx measure fit-body FILE --model PATH [--out REPORT | --mesh-out NEW_DIR]
      [--iterations N] [--max-evaluations N] [--silhouettes MANIFEST] [--json]
  ftx measure capture-review FILE --out NEW_DIR [--silhouettes MANIFEST] [--json]
  ftx measure capture-init-review MANIFEST --model MODEL --calibration CAL --reference CHARUCO_DECL
      --reference-id ID --out NEW_DIR [--json]
  ftx measure capture-init CHOICES --model MODEL --out NEW_DIR [--json]
  ftx measure body-target-review FILE --model MODEL --out NEW_DIR [--json]
  ftx measure body-targets TARGETS --model MODEL --out NEW_DIR [--json]
  ftx measure fit-start-review FILE --model MODEL --out NEW_DIR [--silhouettes MANIFEST] [--json]
  ftx measure fit-starts ANNOTATIONS --model MODEL --out NEW_DIR [--json]
  ftx measure still-window FILE [--out FILE] [--json]
  ftx measure video-select FILE [--out FILE] [--json]
  ftx measure video-decode REQUEST.json --out NEW_DIR [--json]
  ftx measure video-mask-review MANIFEST.json --out NEW_DIR [--json]
  ftx measure video-masks POLYGONS.json --out NEW_DIR [--json]
  ftx measure fuse FILES... [--no-priors] [--out FILE] [--json]
  ftx measure ready FILES... --garment KIND [--as-of DATE] [--no-priors] [--closed-cuff] [--passes-hip]
      [--nappies|--no-nappies] [--elastic-waist] [--cuff-level knee|calf|ankle] [--cuff-opening] [--out FILE] [--json]
  ftx export TARGET PATTERN --out DIR [--unfold] [--sa MM] [--mitre-limit N] [--projector-width MM]
            [--layout-width MM] [--keep-blank-pages] [--hpgl-rotate90] [--hpgl-mirror] [--created YYYY-MM-DDTHH:MM] [--json]
      TARGET: svg pdf letter a4 copyshop projector dxf astm aama flat-dxf hpgl cutter garmentcode embroidery all
  ftx check DIR [--pattern-ref PATTERN] [--json]
  ftx trace IMAGE.png|.jpg|.webp --out DIR [--crop X,Y,W,H] [--channel auto|bgdist|dark|light|gray|sat|r|g|b|edge]
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
            if name == "allow-estimates"
                || (name == "check-contact" && argv.first().is_some_and(|s| s == "viewer"))
                || name == "static-equilibrium"
                || name == "time-subdivision"
                || measure::BOOLEAN_OPTIONS.contains(&name)
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

/// Recompute record-bearing inputs; explicit fused-only/flat callers retain their supplied targets.
/// Keep the original bytes separately for source identity, never replace them with a fused serialization.
fn current_measurements(raw: Value) -> Result<Value, Refusal> {
    if raw.get("records").is_some() {
        round::measurements(&raw)
    } else {
        Ok(raw)
    }
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
    let fit = fit_tape(model, &w, &tape_targets(meas)?, &Pose::drape())?;
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
        w.weight_kg.map_or_else(String::new, |kg| {
            let source = if meas["wearer"]["weight_kg"].is_null() {
                "BMI-derived target"
            } else {
                "supplied weight"
            };
            format!(
                " ({source} {kg:.2} kg, residual {:+.3} kg)",
                fit.mass_kg - kg
            )
        })
    );
    Ok((w, fit))
}

/// The fitted body as `drape` reports it and `body` writes it.
fn body_json(w: &Wearer, fit: &TapeFit, measurements: &Value) -> serde_json::Map<String, Value> {
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
                "residual_mm": r.body_mm - r.target_mm,
                "sigma_mm": r.sigma_mm,
                "effective_sigma_mm": r.effective_sigma_mm,
                "normalized_residual": (r.body_mm - r.target_mm) / r.effective_sigma_mm,
            })))),
            "free": named(&mut FREE.iter().zip(fit.free).map(|(n, v)| {
                ((*n).to_string(), json!((v * 1e4).round() / 1e4))
            })),
            "weight_kg": fit.mass_comparison(w, measurements).map(|mut row| {
                row["body_kg"] = json!(kg(fit.mass_kg));
                row
            }),
        }),
    );
    m.insert(
        "readings_mm".into(),
        Value::Object(named(
            &mut TAPE_FIELDS
                .iter()
                .zip(fit.readings)
                .filter_map(|(n, v)| v.map(|v| ((*n).to_string(), json!(mm(v))))),
        )),
    );
    let unavailable = fit.unavailable_fields();
    m.insert(
        "tape_evaluation".into(),
        json!({"status":if unavailable.is_empty() { "AVAILABLE" } else { "PARTIAL" },
            "pose":Pose::TAPE_FIT_POSE_NAME,"provenance":"MODEL_FIT","unavailable_fields":unavailable}),
    );
    m.insert("mass_kg".into(), json!(kg(fit.mass_kg)));
    m.insert(
        "model_fit".into(),
        Value::Array(ftx_measure::model_fit_records(
            measurements,
            TAPE_FIELDS.into_iter().zip(fit.readings),
            fit.residuals.iter().map(|r| r.name.as_str()),
        )),
    );
    m
}

pub(crate) fn parse_skinning(args: &Args) -> Result<SkinningMode, Refusal> {
    match args.options.get("skinning").map(String::as_str) {
        None | Some("lbs") => Ok(SkinningMode::LinearBlend),
        Some("dqs") => Ok(SkinningMode::DualQuaternion),
        Some(other) => Err(
            bad_input(format!("--skinning {other}: no such skinning mode"))
                .repair("--skinning lbs or dqs"),
        ),
    }
}

/// `--hands wrist|palm`: how much of each hand the clothing collider keeps. The default is the
/// reference's collider, capped at the wrists; `palm` is the native one that keeps the palms.
pub(crate) fn parse_hands(args: &Args) -> Result<HandCollider, Refusal> {
    match args.options.get("hands").map(String::as_str) {
        None => Ok(HandCollider::Wrist),
        Some(value) => HandCollider::parse(value).ok_or_else(|| {
            bad_input(format!("--hands {value}: no such hand collider"))
                .repair("--hands wrist or palm")
        }),
    }
}

/// `ftx body`: the body fitted to a measurement file, posed, as `body.obj` and `body.json`
/// (`ftx-body/1`: the fit, the readings, the phenotype and the local changes in use).
fn cmd_body(args: &Args) -> Result<(Value, i32, String), Refusal> {
    if args.options.contains_key("body-fit") {
        return body_source::cmd_body(args);
    }
    known_options(
        args,
        "body",
        &["measurements", "out", "pose", "skinning", "model"],
    )?;
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
    let skinning = parse_skinning(args)?;
    let (meas, meas_bytes) = read_json(required(args, "measurements")?)?;
    let meas = current_measurements(meas)?;
    let out = PathBuf::from(required(args, "out")?);
    reserve_output(&out, "body")?;
    let model = load_model(args)?;
    let (w, fit) = fitted_body(&model, &meas)?;
    let occupancy = ftx_body::nappy_occupancy(&model, &fit.params, &meas)?;
    let params = occupancy
        .as_ref()
        .map_or(&fit.params, |occupied| &occupied.params);
    let rest = model.rest(params)?;
    let posed =
        model.pose_with_skinning(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?, skinning)?;
    let skin = skin_mesh(&posed.vertices, &model.data().faces)?;
    let pose_geometry = ftx_body::body_pose_geometry_json_with_skinning(
        &model,
        &rest.vertices,
        &posed.vertices,
        &skin,
        skinning,
    )?;
    let ph = fit.params.phenotype;
    let labels = &model.data().header.labels.local_changes;
    let mut doc = body_json(&w, &fit, &meas);
    if let Some(occupied) = occupancy {
        doc.entry("readings_mm").or_insert_with(|| json!({}))["seat_over_nappy"] =
            json!(occupied.seat_over_nappy_mm);
        doc.insert("nappy_occupancy".into(), occupied.description);
    }
    doc.insert("schema".into(), json!("ftx-body/1"));
    doc.insert("measurements_sha256".into(), json!(sha256_hex(&meas_bytes)));
    doc.insert("pose".into(), json!(pose_name));
    if skinning != SkinningMode::LinearBlend {
        doc.insert("skinning".into(), json!(skinning.as_str()));
    }
    doc.insert("pose_geometry".into(), pose_geometry);
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
        workspace::write_new(&out.join(name), bytes)
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
    let skinning_note = if skinning == SkinningMode::LinearBlend {
        String::new()
    } else {
        format!(", skinning {}", skinning.as_str())
    };
    let text = format!(
        "{} mm, {:.2} kg, {} tape measurements fitted (largest residual {:.1} mm); pose {pose_name}{skinning_note}\nwrote {}",
        (fit.readings[0].expect("stature is always fitted") * 10.0).round() / 10.0,
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
    let (doc, bytes) = read_json(pattern_path)?;
    // the input in its key order: alter.py writes a deep copy with the edits in place
    let ordered = std::str::from_utf8(&bytes)
        .map_err(|e| e.to_string())
        .and_then(ftx_export::pyjson::PyJson::parse)
        .map_err(|e| bad_input(format!("{pattern_path}: {e}")))?;
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
    // The bytes are alter.py's: its key order, json.dumps(…, indent=1).
    let target = args.options.get("to").and_then(|t| t.split_once('/'));
    let (piece, item) = match op {
        "transfer-curve" => (target.map(|t| t.0), target.map(|t| t.1)),
        "move" => (
            args.options.get("piece").map(String::as_str),
            args.options.get("vertex").map(String::as_str),
        ),
        _ => (args.options.get("piece").map(String::as_str), None),
    };
    let body = ftx_trace::alter_py(&ordered, &result, op, piece, item).dumps(Some(1)) + "\n";
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

struct SelectedFlatBody {
    body: ftx_drape::drafting::PreparedPatternBody,
    source: Option<Vec<u8>>,
}

fn selected_flat_body(
    args: &Args,
    pattern: &Value,
    measurements: &Value,
    spec: &Value,
    selected: bool,
) -> Result<Option<SelectedFlatBody>, Refusal> {
    if !selected {
        if pattern["meta"].get("draft_body").is_some() {
            return Err(bad_input(
                "this pattern was drafted on a selected body; supply --body-fit REPORT and/or --model PATH (or --flat-body for the default model) to validate on an explicit body source",
            ));
        }
        return Ok(None);
    }
    let (model, hash, source) = body_source::selected_inputs(args)?;
    let accepted = source
        .as_deref()
        .map(std::str::from_utf8)
        .transpose()
        .map_err(|e| bad_input(e.to_string()))?;
    let passage: Vec<_> = ["hand_pass", "foot_pass"]
        .into_iter()
        .filter(|name| {
            spec["points"]
                .as_array()
                .into_iter()
                .flatten()
                .any(|point| point["body"].as_str() == Some(*name))
        })
        .collect();
    let body = ftx_drape::drafting::PreparedPatternBody::prepare(
        &model,
        &hash,
        measurements,
        accepted,
        &passage,
    )?;
    Ok(Some(SelectedFlatBody { body, source }))
}

fn cmd_validate(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "validate",
        &["measurements", "fit", "model", "body-fit", "flat-body"],
    )?;
    let pattern_path = args
        .positional
        .get(1)
        .ok_or_else(|| bad_input("ftx validate needs a pattern file"))?;
    let (pat, _) = read_json(pattern_path)?;
    let (meas, _) = read_json(required(args, "measurements")?)?;
    let spec = fit_spec(args)?;
    let selected = ["model", "body-fit", "flat-body"]
        .iter()
        .any(|key| args.options.contains_key(*key));
    let meas = if selected {
        current_measurements(meas)?
    } else {
        meas
    };
    let body = selected_flat_body(args, &pat, &meas, &spec, selected)?;
    let pattern = Pattern::new(pat);
    let rep = if let Some(selected) = &body {
        validate(&pattern, &spec, &selected.body)
    } else {
        validate(&pattern, &spec, &Measurements(meas))
    };
    let mut result = serde_json::to_value(&rep).map_err(|e| bad_input(e.to_string()))?;
    let mut text = format_table(&rep);
    if let Some(selected) = body {
        result["validation_body"] = selected.body.evidence();
        text.push_str("\nFlat checks use MODEL_FIT dimensions in the source measurement pose.");
    }
    Ok((result, rep.exit_code(), text))
}

/// Validate settings before reading files, fitting the body or creating outputs.
fn drape_options(args: &Args) -> Result<DrapeOptions, Refusal> {
    let mut opts = DrapeOptions::default();
    if let Some(seconds) = args.options.get("time-step-s") {
        opts.step = seconds
            .parse()
            .map_err(|_| bad_input("--time-step-s must be a positive duration in seconds"))?;
    }
    if let Some(tolerance) = args.options.get("newton-tol-m-s") {
        opts.tol = tolerance.parse().map_err(|_| {
            bad_input("--newton-tol-m-s must be a positive correction speed in m/s")
        })?;
    }
    if let Some(mm) = args.options.get("spacing-mm") {
        opts.spacing = mm
            .parse()
            .map_err(|_| bad_input("--spacing-mm must be a positive distance in millimetres"))?;
    }
    if let Some(factor) = args.options.get("compression") {
        opts.compression = factor.parse().map_err(|_| {
            bad_input("--compression must be a fraction of the tensile stiffness in (0, 1]")
        })?;
    }
    for (name, slot) in [
        ("sew", &mut opts.sew_steps),
        ("drape", &mut opts.drape_steps),
        ("max-settle-steps", &mut opts.max_settle_steps),
        ("elastic-hold-steps", &mut opts.elastic_hold_steps),
    ] {
        if let Some(v) = args.options.get(name) {
            *slot = v
                .parse()
                .map_err(|_| bad_input(format!("--{name} must be a whole number")))?;
        }
    }
    if args.options.contains_key("static-equilibrium") {
        let mut policy = StaticEquilibriumOptions::default();
        if let Some(value) = args.options.get("static-tol-mm") {
            policy.correction_tolerance_m = value.parse::<f64>().map_err(|_| {
                bad_input("--static-tol-mm must be a positive distance in millimetres")
            })? * 1e-3;
        }
        if let Some(value) = args.options.get("static-max-newton") {
            policy.max_newton_iterations = value
                .parse()
                .map_err(|_| bad_input("--static-max-newton must be a whole number in 1..=1000"))?;
        }
        opts.static_equilibrium = Some(policy);
    } else if let Some(name) = ["static-tol-mm", "static-max-newton"]
        .into_iter()
        .find(|key| args.options.contains_key(*key))
    {
        return Err(bad_input(format!("--{name} requires --static-equilibrium")));
    }
    if args.options.contains_key("time-subdivision") {
        let mut policy = SubdivisionOptions::default();
        for (name, slot) in [
            ("subdivision-max-halvings", &mut policy.max_halvings),
            ("subdivision-max-trials", &mut policy.max_trials),
        ] {
            if let Some(value) = args.options.get(name) {
                *slot = value
                    .parse()
                    .map_err(|_| bad_input(format!("--{name} must be a whole number")))?;
            }
        }
        opts.time_subdivision = Some(policy);
    } else if let Some(name) = ["subdivision-max-halvings", "subdivision-max-trials"]
        .into_iter()
        .find(|key| args.options.contains_key(*key))
    {
        return Err(bad_input(format!("--{name} requires --time-subdivision")));
    }
    // Placement is also used to inspect failed starts. Physical garment runs always admit
    // cloth contact, including when a caller deliberately chooses zero contact thickness.
    let physical = matches!(
        args.positional.first().map(String::as_str),
        Some("drape" | "round")
    );
    let thickness = args
        .options
        .get("self-contact-mm")
        .map(String::as_str)
        .or(physical.then_some("2"));
    if let Some(mm) = thickness {
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
    if let Some(strategy) = args.options.get("newton-strategy") {
        opts.newton_strategy = match strategy.as_str() {
            "projected" => NewtonStrategy::ProjectedNewton,
            "adaptive-ilu" => NewtonStrategy::adaptive_ilu_default(),
            "trust-region" => NewtonStrategy::trust_region_default(),
            _ => {
                return Err(bad_input(
                    "--newton-strategy must be projected, adaptive-ilu or trust-region",
                ));
            }
        };
    }
    if let Some(integrator) = args.options.get("integrator") {
        opts.integrator = match integrator.as_str() {
            "implicit-euler" => {
                if args.options.contains_key("rho-inf") {
                    return Err(bad_input(
                        "--rho-inf requires --integrator generalized-alpha",
                    ));
                }
                TimeIntegrator::ImplicitEuler
            }
            "generalized-alpha" => {
                let rho_inf = match args.options.get("rho-inf") {
                    Some(v) => v.parse::<f64>().map_err(|_| {
                        bad_input("--rho-inf must be a finite spectral radius in [0, 1]")
                    })?,
                    None => 0.0,
                };
                TimeIntegrator::GeneralizedAlpha { rho_inf }
            }
            _ => {
                return Err(bad_input(
                    "--integrator must be implicit-euler or generalized-alpha",
                ));
            }
        };
    } else if args.options.contains_key("rho-inf") {
        return Err(bad_input(
            "--rho-inf requires --integrator generalized-alpha",
        ));
    }
    opts.validate()?;
    Ok(opts)
}

/// Pose the same fitted body; do not change its tape-fit reference pose.
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
    if let Some(value) = args.options.get("legs") {
        pose.legs = match value.as_str() {
            "standing" => Legs::Standing,
            "seated" => Legs::Seated,
            "squat" => Legs::Squat,
            _ => return Err(bad_input("--legs must be standing, seated or squat")),
        };
    }
    if let Some(value) = args.options.get("leg-fraction") {
        pose.leg_fraction = value
            .parse()
            .map_err(|_| bad_input("--leg-fraction must be a finite number in 0..=1"))?;
    }
    pose.validate()?;
    Ok(pose)
}

/// Claim a fresh destination before fitting/solving. A failed retry must not leave a previous
/// successful run at the path the caller will inspect for its new result.
fn reserve_drape_output(out: &std::path::Path) -> Result<(), Refusal> {
    reserve_output(out, "drape")
}

fn reserve_output(out: &std::path::Path, command: &str) -> Result<(), Refusal> {
    match std::fs::symlink_metadata(out) {
        Ok(_) => {
            return Err(bad_input(format!(
                "{}: {command} output already exists; use a new directory for every run",
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
    cmd_drape_bound(args, None)
}

fn cmd_drape_bound(
    args: &Args,
    expected_model_sha256: Option<&str>,
) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "drape",
        &[
            "measurements",
            "out",
            "fit",
            "sew",
            "drape",
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
            "model",
            "time-step-s",
            "newton-tol-m-s",
            "self-contact-mm",
            "spacing-mm",
            "compression",
            "self-mu",
            "elastic-ea",
            "elastic-hold-steps",
            "arm-deg",
            "legs",
            "leg-fraction",
            "skinning",
            "hands",
            "body-fit",
            "two-start-sew",
            "fabric",
            "spread-vertex-mm",
            "spread-hem-mm",
            "spread-clearance-mm",
            "spread-seam-pp",
        ],
    )?;
    let opts = drape_options(args)?;
    let pair = two_start::options(args, &opts)?;
    let pose = drape_pose(args)?;
    let skinning = parse_skinning(args)?;
    let hands = parse_hands(args)?;
    let pattern_path = args
        .positional
        .get(1)
        .ok_or_else(|| bad_input("ftx drape needs a pattern file"))?;
    let (mut pat, pattern_bytes) = read_json(pattern_path)?;
    let fabric_override = fabric::load(args)?;
    let outfit = if pat["schema"] == ftx_drape::outfit::SCHEMA {
        if fabric_override.is_some() || pair.is_some() {
            return Err(bad_input(
                "joint outfit requests do not accept --fabric or --two-start-sew; retain explicit source selections and one joint start",
            ));
        }
        let input =
            serde_json::from_value(pat.clone()).map_err(|e| bad_input(format!("outfit: {e}")))?;
        let prepared = ftx_drape::outfit::prepare(&input, &opts)?;
        pat = prepared.pattern().raw().clone();
        Some(prepared)
    } else {
        None
    };
    if let Some(selected) = &fabric_override {
        selected.apply(&mut pat)?;
    }
    let material_probe = Pattern::new(pat.clone());
    ftx_drape::garment::simulation_scope(&material_probe)?;
    let material = ftx_drape::material::resolve(&material_probe, &opts)?;
    let meas_path = required(args, "measurements")?;
    let (meas, meas_bytes) = read_json(meas_path)?;
    let meas = current_measurements(meas)?;
    let out = PathBuf::from(required(args, "out")?);
    let spec = fit_spec(args)?;
    // Refuse unwritable/existing destinations before the expensive fit and drape.
    reserve_drape_output(&out)?;
    let body_source::PreparedBody {
        model,
        params,
        description,
        source_bytes,
    } = body_source::prepare_bound(args, Some(&meas), expected_model_sha256)?;
    let rest = model.rest(&params)?;
    let rest_skin = clothing_skin_with_hands(&model, &rest.vertices, hands)?;
    let posed =
        model.pose_with_skinning(&rest, &pose_deltas(&model, &rest, &pose, 1.0)?, skinning)?;
    let skin = clothing_skin_with_hands(&model, &posed.vertices, hands)?;
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
    let mut refused = None;
    let mut run = if let Some(prepared) = &outfit {
        prepared.drape_observed(
            &spec,
            &body,
            &opts,
            |line| eprintln!("{line}"),
            &mut refused,
        )
    } else if let Some(policy) = pair {
        ftx_drape::two_start::drape_observed(
            &pattern,
            &spec,
            &body,
            &opts,
            &policy,
            |line| eprintln!("{line}"),
            &mut refused,
        )
    } else {
        drape_observed(
            &pattern,
            &spec,
            &body,
            &opts,
            |line| eprintln!("{line}"),
            &mut refused,
        )
    }
    .map_err(|refusal| match refused.as_ref() {
        Some(state) => place::refused_stills(
            refusal,
            Some(state),
            &out,
            obj(&full.vertices, &full.triangles, None).as_bytes(),
        ),
        // No solver state: the start itself was refused. Draw it from what the solver was given.
        None if refusal.code == RefusalCode::IntersectingStart => place::start_stills(
            refusal,
            &place::Staging {
                pattern: &pattern,
                label: pattern_path,
                spacing: material.options.spacing,
                elastic_ea: material.options.elastic_ea,
                frames: &frames,
                collision: (&skin.vertices, &skin.triangles),
                display: (&full.vertices, &full.triangles),
                skinning,
                hands,
            },
            &out,
        ),
        None => refusal,
    })?;
    run.report["body_contact_proxy"] =
        skin.contact_proxy_basis_with_rest_and_skinning(&rest_skin, skinning);
    let mut report = run.report.clone();
    // Recorded only when it is not the reference collider, as the skinning mode is.
    if !hands.is_wrist() {
        report["hand_collider"] = json!(hands.as_str());
    }
    report["qualification_basis"] = ftx_drape::qualify::qualification_basis(&body, &material)?;
    if outfit.is_some() {
        report["outfit"]["input_file_sha256"] = json!(sha256_hex(&pattern_bytes));
    }
    if let Some(selected) = &fabric_override {
        report["fabric_override"] = selected.receipt().clone();
    }
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
        if !pose.legs.is_standing() {
            b.insert("legs".into(), json!(pose.legs));
        }
        if pose.leg_fraction != 1.0 {
            b.insert("leg_fraction".into(), json!(pose.leg_fraction));
        }
        if skinning != SkinningMode::LinearBlend {
            b.insert("skinning".into(), json!(skinning.as_str()));
        }
        o.insert("body".into(), Value::Object(b));
    }
    let html = viewer(&run, &pattern, &body, VIEWER_TEMPLATE).map_err(bad_input)?;
    // the last frame and the body as OBJ, for renders outside (the skill's Blender stills): the
    // garment once with its seam colours and once with its strain colours (`v x y z r g b`)
    let mesh = &run.scene.mesh;
    let last = run.fold.frames.last().map_or(&[][..], Vec::as_slice);
    let style = pattern.raw()["style"].as_str().unwrap_or("");
    let mut files = vec![
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
    ];
    if let Some(source) = source_bytes {
        files.push(("body_fit.json", source));
    }
    if report["static_equilibrium"]["status"] == "available" {
        let equilibrium = &report["static_equilibrium"]["result"];
        let vertices: Vec<Vec3> = serde_json::from_value(equilibrium["positions_m"].clone())
            .map_err(|e| bad_input(e.to_string()))?;
        let triangles: Vec<[usize; 3]> = serde_json::from_value(equilibrium["triangles"].clone())
            .map_err(|e| bad_input(e.to_string()))?;
        files.push((
            "static_equilibrium.obj",
            obj(&vertices, &triangles, None).into_bytes(),
        ));
    }
    if outfit.is_some() {
        files.push((
            "outfit_pattern.json",
            serde_json::to_vec_pretty(pattern.raw()).map_err(|e| bad_input(e.to_string()))?,
        ));
        files.push(("outfit_request.json", pattern_bytes));
    }
    write_drape_output(&out, &report, &files)?;
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
    text.push_str(match report["numerical"]["settled"].as_bool() {
        Some(true) => "numerical: Newton converged; instantaneous-speed quiet window passed\n",
        Some(false) => "numerical: Newton converged; instantaneous-speed quiet window failed\n",
        None => "numerical: Newton converged; gravity settling not evaluated\n",
    });
    text.push_str(if opts.self_contact.is_some() {
        "self-contact: enabled; solver-state audits passed\n"
    } else {
        "self-contact: not simulated\n"
    });
    if let Some(equilibrium) = report.get("static_equilibrium") {
        if equilibrium["status"] == "available" {
            text.push_str(&format!(
                "static equilibrium: independent frictionless comparison converged in {} Newton iterations; correction {} m, residual {} N; dynamic run unchanged\n",
                equilibrium["result"]["solve"]["iterations"],
                equilibrium["result"]["solve"]["last_step"],
                equilibrium["result"]["solve"]["residual"],
            ));
        } else {
            text.push_str(&format!(
                "static equilibrium: refused ({}); dynamic run retained\n",
                equilibrium["refusal"]["message"]
                    .as_str()
                    .unwrap_or("unavailable"),
            ));
        }
    }
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
        "numerical": report["numerical"],
        "self_contact": report["self_contact"],
        "files": std::iter::once("sim_report.json")
            .chain(files.iter().map(|(name, _)| *name)).collect::<Vec<_>>(),
    });
    if let Some(pair) = report.get("two_start") {
        result["two_start"] = pair.clone();
    }
    if let Some(equilibrium) = report.get("static_equilibrium") {
        result["static_equilibrium"] = equilibrium.clone();
    }
    if let Some(composition) = report.get("outfit") {
        result["outfit"] = composition.clone();
        result["files"] = json!(
            files
                .iter()
                .map(|(name, _)| *name)
                .chain(std::iter::once("sim_report.json"))
                .collect::<Vec<_>>()
        );
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
        Some("import") => emit("import", args.json, import::cmd_import(&args)),
        Some("draft") => emit("draft", args.json, draft::cmd_draft(&args)),
        Some("place") => emit("place", args.json, place::run(&args)),
        Some("render") => emit("render", args.json, render::cmd_render(&args)),
        Some("inspect-pixel") => emit("inspect-pixel", args.json, inspect_pixel::run(&args)),
        Some("viewer") => emit("viewer", args.json, fold::cmd_viewer(&args)),
        Some("report") => emit("report", args.json, round::cmd_report(&args)),
        Some("sim-points") => emit("sim-points", args.json, fold::sim_points(&args)),
        Some("outfit") => emit("outfit", args.json, outfit::cmd_outfit(&args)),
        Some("validate") => emit("validate", args.json, cmd_validate(&args)),
        Some("round") => emit("round", args.json, round::cmd_round(&args)),
        Some("qualify") => emit("qualify", args.json, round::cmd_qualify(&args)),
        Some("drape") => emit("drape", args.json, cmd_drape(&args)),
        Some("body") => emit("body", args.json, cmd_body(&args)),
        Some("alter") => emit("alter", args.json, cmd_alter(&args)),
        Some("measure") => emit("measure", args.json, measure::run(&args)),
        Some("materials") => emit("materials", args.json, materials::run(&args)),
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

    #[test]
    fn body_report_retains_fit_uncertainty_and_submillimetre_residuals() {
        let wearer = Wearer {
            years: 2.5,
            gender: 0.5,
            stature_mm: 920.0,
            weight_kg: Some(10.0001),
        };
        let fit = TapeFit {
            params: ftx_body::BodyParams {
                phenotype: Default::default(),
                local_changes: vec![],
            },
            readings: std::array::from_fn(|i| if i == 16 { None } else { Some(100.0004) }),
            mass_kg: 10.0003,
            residuals: vec![ftx_body::TapeResidual {
                name: "waist".into(),
                target_mm: 100.0,
                body_mm: 100.0004,
                sigma_mm: Some(17.0),
                effective_sigma_mm: 17.0,
            }],
            free: [0.0; FREE.len()],
            iterations: 3,
        };
        let body = Value::Object(body_json(
            &wearer,
            &fit,
            &json!({"wearer":{"weight_kg":10.0001,"bmi":16}}),
        ));
        let mass = &body["fit"]["weight_kg"];
        assert_eq!(mass["target_source"], "wearer.weight_kg");
        assert_eq!(mass["body_kg"], 10.0);
        assert_eq!(mass["residual_kg"], 10.0003 - 10.0001);
        assert_eq!(
            mass["normalized_residual"],
            (10.0003 - 10.0001) / (10.0001 * 0.01)
        );
        let row = &body["fit"]["tape"]["waist"];
        assert_eq!(row["body_mm"], 100.0); // Preserve the historical display precision.
        assert_eq!(row["residual_mm"], 100.0004 - 100.0);
        assert_eq!(row["sigma_mm"], 17.0);
        assert_eq!(row["effective_sigma_mm"], 17.0);
        assert_eq!(row["normalized_residual"], (100.0004 - 100.0) / 17.0);
        assert_eq!(body["tape_evaluation"]["status"], "PARTIAL");
        assert_eq!(
            body["tape_evaluation"]["unavailable_fields"],
            json!(["head"])
        );
        assert!(body["readings_mm"].get("head").is_none());
        assert!(body["model_fit"].as_array().unwrap().iter().all(|record| {
            record["name"] != "waist"
                && record["name"] != "head"
                && record["provenance"] == "MODEL_FIT"
        }));
    }

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

    fn placed_drape_pattern() -> Value {
        json!({
            "schema":"fabric-pattern/1", "units":"mm",
            "pieces":[{
                "id":"panel", "cut":{"qty":1},
                "vertices":{"a":[0,0],"b":[100,0],"c":[100,200],"d":[0,200]},
                "edges":[
                    {"id":"bottom","from":"a","to":"b"},
                    {"id":"right","from":"b","to":"c"},
                    {"id":"top","from":"c","to":"d"},
                    {"id":"left","from":"d","to":"a"}
                ],
                "placement":{"part":"torso"}
            }],
            "seams":[]
        })
    }

    #[test]
    fn drape_refuses_unplaced_attachments_before_body_loading_or_output_creation() {
        let dir = drape_scratch();
        let out = dir.join("run");
        let mut pattern = placed_drape_pattern();
        let mut missing = pattern["pieces"][0].clone();
        missing["id"] = json!("unplaced");
        missing["placement"] = Value::Null;
        pattern["pieces"].as_array_mut().unwrap().push(missing);
        pattern["seams"] = json!([{"id":"attachment","a":"panel/right","b":"unplaced/left"}]);
        std::fs::write(
            dir.join("pattern.json"),
            serde_json::to_vec(&pattern).unwrap(),
        )
        .unwrap();
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
        let refusal = cmd_drape(&args).unwrap_err();
        assert_eq!(refusal.code, RefusalCode::InvalidDocument);
        assert!(refusal.message.contains("unplaced"));
        assert!(refusal.message.contains("attachment"));
        assert!(!out.exists());
    }

    #[test]
    fn tape_body_refuses_existing_outputs_before_loading_the_model() {
        let dir = drape_scratch();
        let out = dir.join("body");
        std::fs::create_dir(&out).unwrap();
        let previous = br#"{"schema":"ftx-body/1","previous_run":true}"#;
        std::fs::write(out.join("body.json"), previous).unwrap();
        let source = dir.join("measurements.json");
        std::fs::write(&source, b"{}").unwrap();
        let mut args = parse(&[
            "body".into(),
            "--measurements".into(),
            source.to_string_lossy().into_owned(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
            "--model".into(),
            dir.join("absent.ftxbody").to_string_lossy().into_owned(),
        ])
        .unwrap();
        let destinations = vec![out.clone(), source.clone()];
        #[cfg(unix)]
        let destinations = {
            use std::os::unix::fs::symlink;
            let mut destinations = destinations;
            let alias = dir.join("alias");
            symlink(&out, &alias).unwrap();
            destinations.push(alias);
            let dangling = dir.join("dangling");
            symlink("no-such-target", &dangling).unwrap();
            destinations.push(dangling);
            destinations
        };
        for destination in destinations {
            args.options
                .insert("out".into(), destination.to_string_lossy().into_owned());
            let error = cmd_body(&args).unwrap_err();
            assert_eq!(error.code, RefusalCode::InvalidDocument, "{error}");
            assert!(
                error.message.contains("body output already exists"),
                "{error}"
            );
            assert_eq!(std::fs::read(out.join("body.json")).unwrap(), previous);
            assert_eq!(std::fs::read(&source).unwrap(), b"{}");
            assert_eq!(std::fs::read_dir(&out).unwrap().count(), 1);
        }
        assert!(!dir.join("no-such-target").exists());
    }

    #[test]
    fn drape_refuses_existing_outputs_before_loading_the_body() {
        let dir = drape_scratch();
        let out = dir.join("run");
        std::fs::create_dir(&out).unwrap();
        let previous = br#"{"settled":true,"gating":[],"previous_run":true}"#;
        std::fs::write(out.join("sim_report.json"), previous).unwrap();
        std::fs::write(
            dir.join("pattern.json"),
            serde_json::to_vec(&placed_drape_pattern()).unwrap(),
        )
        .unwrap();
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
    fn leg_pose_and_settling_controls_are_independent_and_validate_before_io() {
        let args = parse(
            &[
                "drape",
                "--arm-deg",
                "50",
                "--legs",
                "seated",
                "--leg-fraction",
                "0.5",
                "--max-settle-steps",
                "24",
            ]
            .map(String::from),
        )
        .unwrap();
        let pose = drape_pose(&args).unwrap();
        assert_eq!(pose.arm_abduction_deg, Some(50.0));
        assert_eq!(pose.legs, Legs::Seated);
        assert_eq!(pose.leg_fraction, 0.5);
        assert_eq!(drape_options(&args).unwrap().max_settle_steps, 24);
        for (flag, value) in [
            ("legs", "kneeling"),
            ("leg-fraction", "NaN"),
            ("leg-fraction", "-0.1"),
            ("leg-fraction", "1.1"),
            ("max-settle-steps", "1.5"),
            ("max-settle-steps", "257"),
        ] {
            let args = parse(&[
                "drape".into(),
                "absent.json".into(),
                format!("--{flag}"),
                value.into(),
            ])
            .unwrap();
            assert_ne!(cmd_drape(&args).unwrap_err().code, RefusalCode::Io);
        }
    }

    #[test]
    fn time_subdivision_controls_are_explicit_and_validate_before_io() {
        let args = parse(
            &[
                "drape",
                "--time-subdivision",
                "--subdivision-max-halvings",
                "3",
                "--subdivision-max-trials",
                "200",
            ]
            .map(String::from),
        )
        .unwrap();
        let options = drape_options(&args).unwrap();
        assert_eq!(
            options.time_subdivision,
            Some(SubdivisionOptions {
                max_halvings: 3,
                max_trials: 200,
            })
        );
        assert_eq!(options.step, DrapeOptions::default().step);
        assert_eq!(options.tol, DrapeOptions::default().tol);
        assert!(
            drape_options(&parse(&["drape".into()]).unwrap())
                .unwrap()
                .time_subdivision
                .is_none()
        );
        for extra in [
            vec!["--subdivision-max-halvings", "3"],
            vec!["--subdivision-max-trials", "200"],
            vec!["--time-subdivision", "--subdivision-max-halvings", "17"],
            vec!["--time-subdivision", "--subdivision-max-halvings", "-1"],
            vec!["--time-subdivision", "--subdivision-max-trials", "0"],
            vec!["--time-subdivision", "--subdivision-max-trials", "65536"],
            vec!["--time-subdivision", "--subdivision-max-trials", "1.5"],
            vec!["--time-subdivision", "--subdivision-max-trials", "1"],
        ] {
            let mut argv = vec!["drape", "absent.json"];
            argv.extend(extra);
            let args = parse(&argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            assert_ne!(cmd_drape(&args).unwrap_err().code, RefusalCode::Io);
        }
        let args = parse(&["round", "absent", "--time-subdivision"].map(String::from)).unwrap();
        assert!(
            round::cmd_round(&args)
                .unwrap_err()
                .message
                .contains("requires --sim")
        );
    }

    #[test]
    fn static_equilibrium_controls_are_explicit_and_validate_before_io() {
        let args = parse(
            &[
                "drape",
                "--static-equilibrium",
                "--static-tol-mm",
                "0.002",
                "--static-max-newton",
                "120",
            ]
            .map(String::from),
        )
        .unwrap();
        let options = drape_options(&args).unwrap();
        assert_eq!(
            options.static_equilibrium,
            Some(StaticEquilibriumOptions {
                correction_tolerance_m: 2e-6,
                max_newton_iterations: 120,
            })
        );
        assert_eq!(options.step, DrapeOptions::default().step);
        assert_eq!(options.tol, DrapeOptions::default().tol);
        assert!(
            drape_options(&parse(&["drape".into()]).unwrap())
                .unwrap()
                .static_equilibrium
                .is_none()
        );
        for extra in [
            vec!["--static-tol-mm", "0.1"],
            vec!["--static-max-newton", "10"],
            vec!["--static-equilibrium", "--static-tol-mm", "0"],
            vec!["--static-equilibrium", "--static-tol-mm", "NaN"],
            vec!["--static-equilibrium", "--static-tol-mm", "-1"],
            vec!["--static-equilibrium", "--static-max-newton", "0"],
            vec!["--static-equilibrium", "--static-max-newton", "1001"],
            vec!["--static-equilibrium", "--static-max-newton", "1.5"],
        ] {
            let mut argv = vec!["drape", "absent.json"];
            argv.extend(extra);
            let args = parse(&argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
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
        assert_eq!(default.self_contact.unwrap().thickness, 0.002);
        let round = drape_options(&parse(&["round".into()]).unwrap()).unwrap();
        assert_eq!(round.self_contact, default.self_contact);
        let place = drape_options(&parse(&["place".into()]).unwrap()).unwrap();
        assert!(place.self_contact.is_none());
        let zero =
            drape_options(&parse(&["drape", "--self-contact-mm", "0"].map(String::from)).unwrap())
                .unwrap();
        assert_eq!(zero.self_contact.unwrap().thickness, 0.0);
    }

    #[test]
    fn nominal_time_step_is_in_seconds_and_validated_before_reading_a_pattern() {
        let args = parse(
            &[
                "drape",
                "--time-step-s",
                "0.001",
                "--newton-tol-m-s",
                "1e-7",
                "--sew",
                "40",
                "--drape",
                "60",
            ]
            .map(String::from),
        )
        .unwrap();
        let opts = drape_options(&args).unwrap();
        assert_eq!(opts.tol, 1e-7);
        assert_eq!(
            (opts.step, opts.sew_steps, opts.drape_steps),
            (0.001, 40, 60)
        );
        for option in ["--time-step-s", "--newton-tol-m-s"] {
            for value in ["0", "-1", "NaN", "inf", "fast"] {
                let args =
                    parse(&["drape", "absent.json", option, value].map(String::from)).unwrap();
                assert_ne!(cmd_drape(&args).unwrap_err().code, RefusalCode::Io);
            }
        }
    }

    #[test]
    fn panel_refinement_spacing_is_explicit_and_rejected_before_io_when_invalid() {
        let args = parse(&["drape", "--spacing-mm", "4"].map(String::from)).unwrap();
        let opts = drape_options(&args).unwrap();
        assert_eq!(opts.spacing, 4.0); // scene meshing is in mm, not metres
        assert_eq!(opts.contact, DrapeOptions::default().contact);
        for value in ["0", "-1", "NaN", "inf", "fine"] {
            let args =
                parse(&["drape", "no-pattern.json", "--spacing-mm", value].map(String::from))
                    .unwrap();
            assert_ne!(cmd_drape(&args).unwrap_err().code, RefusalCode::Io);
        }
    }

    #[test]
    fn the_hand_collider_is_the_reference_wrist_cap_unless_palm_is_asked_for() {
        let none = parse(&["drape".into()]).unwrap();
        assert_eq!(parse_hands(&none).unwrap(), HandCollider::Wrist);
        for (word, expected) in [
            ("wrist", HandCollider::Wrist),
            ("palm", HandCollider::Palm),
        ] {
            let args = parse(&["drape", "--hands", word].map(String::from)).unwrap();
            assert_eq!(parse_hands(&args).unwrap(), expected);
            assert_eq!(expected.as_str(), word);
        }
        // Any other word refuses before a pattern or a body is read, on each command that takes it.
        for command in ["drape", "place"] {
            let args = parse(&[command, "no-pattern.json", "--hands", "mitten"].map(String::from))
                .unwrap();
            let refusal = if command == "drape" {
                cmd_drape(&args).unwrap_err()
            } else {
                place::run(&args).unwrap_err()
            };
            assert_ne!(refusal.code, RefusalCode::Io, "{command}");
            assert!(
                refusal.message.contains("--hands mitten"),
                "{command}: {}",
                refusal.message
            );
            assert_eq!(refusal.repairs, ["--hands wrist or palm"]);
        }
        let round =
            parse(&["round", "no-such-ws", "--sim", "--hands", "mitten"].map(String::from)).unwrap();
        assert!(
            round::cmd_round(&round)
                .unwrap_err()
                .message
                .contains("--hands mitten")
        );
        // Like every other simulation option, it means nothing to a round that does not simulate.
        let flat = parse(&["round", "no-such-ws", "--hands", "palm"].map(String::from)).unwrap();
        assert!(
            round::cmd_round(&flat)
                .unwrap_err()
                .message
                .contains("--hands requires --sim or --stills")
        );
    }

    #[test]
    fn compression_is_the_reference_stiffness_unless_asked_and_refuses_outside_its_range() {
        let default = drape_options(&parse(&["drape".into()]).unwrap()).unwrap();
        assert_eq!(default.compression, 1.0);
        let args = parse(&["drape", "--compression", "0.02"].map(String::from)).unwrap();
        assert_eq!(drape_options(&args).unwrap().compression, 0.02);
        for value in ["0", "-0.1", "1.5", "NaN", "inf", "soft"] {
            let args =
                parse(&["drape", "no-pattern.json", "--compression", value].map(String::from))
                    .unwrap();
            assert_ne!(cmd_drape(&args).unwrap_err().code, RefusalCode::Io);
        }
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
            vec!["--self-mu", "-0.1"],
            vec!["--sew", "0", "--drape", "0"],
            vec!["--elastic-hold-steps", "37"],
            vec!["--elastic-hold-steps", "-1"],
            vec!["--elastic-hold-steps", "1.5"],
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
        let args = parse(&["drape", "--elastic-hold-steps", "2"].map(String::from)).unwrap();
        assert_eq!(drape_options(&args).unwrap().elastic_hold_steps, 2);
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

    #[test]
    fn skinning_option_parses_default_explicit_and_refuses_unknown() {
        let default_args = parse(&["body"].map(String::from)).unwrap();
        assert_eq!(
            parse_skinning(&default_args).unwrap(),
            SkinningMode::LinearBlend
        );
        let lbs_args = parse(&["body", "--skinning", "lbs"].map(String::from)).unwrap();
        assert_eq!(
            parse_skinning(&lbs_args).unwrap(),
            SkinningMode::LinearBlend
        );
        let dqs_args = parse(&["body", "--skinning", "dqs"].map(String::from)).unwrap();
        assert_eq!(
            parse_skinning(&dqs_args).unwrap(),
            SkinningMode::DualQuaternion
        );
        let bad_args = parse(&["body", "--skinning", "bogus"].map(String::from)).unwrap();
        let err = parse_skinning(&bad_args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("--skinning bogus"), "{}", err.message);

        // Both `ftx body` and `ftx drape` must refuse invalid `--skinning` before file I/O or creating outputs.
        let dir = drape_scratch();
        let out = dir.join("body-out");
        let absent_meas = dir.join("absent-measurements.json");
        let body_args = parse(&[
            "body".into(),
            "--measurements".into(),
            absent_meas.to_string_lossy().into_owned(),
            "--out".into(),
            out.to_string_lossy().into_owned(),
            "--skinning".into(),
            "bogus".into(),
            "--model".into(),
            dir.join("absent.ftxbody").to_string_lossy().into_owned(),
        ])
        .unwrap();
        let body_err = cmd_body(&body_args).unwrap_err();
        assert_eq!(body_err.code, RefusalCode::InvalidDocument);
        assert!(body_err.message.contains("--skinning bogus"));
        assert!(!out.exists());

        let meas = dir.join("measurements.json");
        std::fs::write(&meas, b"{}").unwrap();

        let pat = dir.join("pattern.json");
        std::fs::write(&pat, b"{}").unwrap();
        let drape_out = dir.join("drape-out");
        let drape_args = parse(&[
            "drape".into(),
            pat.to_string_lossy().into_owned(),
            "--measurements".into(),
            meas.to_string_lossy().into_owned(),
            "--out".into(),
            drape_out.to_string_lossy().into_owned(),
            "--skinning".into(),
            "bogus".into(),
        ])
        .unwrap();
        let drape_err = cmd_drape(&drape_args).unwrap_err();
        assert_eq!(drape_err.code, RefusalCode::InvalidDocument);
        assert!(!drape_out.exists());

        // `ftx round`, `ftx qualify`, and `ftx place` also enforce `--skinning` upfront.
        let round_no_sim = parse(&[
            "round".into(),
            dir.join("absent-ws").to_string_lossy().into_owned(),
            "--skinning".into(),
            "dqs".into(),
        ])
        .unwrap();
        let round_no_sim_err = round::cmd_round(&round_no_sim).unwrap_err();
        assert_eq!(round_no_sim_err.code, RefusalCode::InvalidDocument);
        assert!(
            round_no_sim_err
                .message
                .contains("--skinning requires --sim or --stills"),
            "{}",
            round_no_sim_err.message
        );

        let round_bad = parse(&[
            "round".into(),
            dir.join("absent-ws").to_string_lossy().into_owned(),
            "--sim".into(),
            "--skinning".into(),
            "bogus".into(),
        ])
        .unwrap();
        let round_bad_err = round::cmd_round(&round_bad).unwrap_err();
        assert_eq!(round_bad_err.code, RefusalCode::InvalidDocument);
        assert!(round_bad_err.message.contains("--skinning bogus"));

        let qual_out = dir.join("qual-out");
        let qual_args = parse(&[
            "qualify".into(),
            "--measurements".into(),
            meas.to_string_lossy().into_owned(),
            "--out".into(),
            qual_out.to_string_lossy().into_owned(),
            "--skinning".into(),
            "bogus".into(),
        ])
        .unwrap();
        let qual_err = round::cmd_qualify(&qual_args).unwrap_err();
        assert_eq!(qual_err.code, RefusalCode::InvalidDocument);
        assert!(qual_err.message.contains("--skinning bogus"));
        assert!(!qual_out.exists());

        let place_out = dir.join("place-out");
        let place_args = parse(&[
            "place".into(),
            pat.to_string_lossy().into_owned(),
            "--measurements".into(),
            meas.to_string_lossy().into_owned(),
            "--out".into(),
            place_out.to_string_lossy().into_owned(),
            "--skinning".into(),
            "bogus".into(),
        ])
        .unwrap();
        let place_err = place::run(&place_args).unwrap_err();
        assert_eq!(place_err.code, RefusalCode::InvalidDocument);
        assert!(place_err.message.contains("--skinning bogus"));
        assert!(!place_out.exists());
    }

    #[test]
    fn newton_strategy_and_integrator_cli_options_parse_and_refuse_fail_closed() {
        let def = drape_options(&parse(&["drape".into()]).unwrap()).unwrap();
        assert_eq!(def.newton_strategy, NewtonStrategy::ProjectedNewton);
        assert_eq!(def.integrator, TimeIntegrator::ImplicitEuler);

        let ilu = drape_options(
            &parse(&["drape", "--newton-strategy", "adaptive-ilu"].map(String::from)).unwrap(),
        )
        .unwrap();
        assert_eq!(ilu.newton_strategy, NewtonStrategy::adaptive_ilu_default());

        let tr = drape_options(
            &parse(
                &[
                    "drape",
                    "--newton-strategy",
                    "trust-region",
                    "--integrator",
                    "generalized-alpha",
                    "--rho-inf",
                    "0.6",
                ]
                .map(String::from),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(tr.newton_strategy, NewtonStrategy::trust_region_default());
        assert_eq!(
            tr.integrator,
            TimeIntegrator::GeneralizedAlpha { rho_inf: 0.6 }
        );

        // Default rho_inf for generalized-alpha is 0.0 (L-stable 2nd-order for stiff IPC contact).
        let ga_def = drape_options(
            &parse(&["drape", "--integrator", "generalized-alpha"].map(String::from)).unwrap(),
        )
        .unwrap();
        assert_eq!(
            ga_def.integrator,
            TimeIntegrator::GeneralizedAlpha { rho_inf: 0.0 }
        );

        // Invalid combinations refuse before file I/O on both `drape` and `round --sim`.
        for bad in [
            vec!["--newton-strategy", "bogus"],
            vec!["--integrator", "verlet"],
            vec!["--rho-inf", "0.5"],
            vec!["--integrator", "implicit-euler", "--rho-inf", "0.5"],
            vec!["--integrator", "generalized-alpha", "--rho-inf", "1.5"],
            vec!["--integrator", "generalized-alpha", "--rho-inf", "NaN"],
        ] {
            let mut argv = vec!["drape", "absent-pattern.json"];
            argv.extend(bad.iter().copied());
            let args = parse(&argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            let err = cmd_drape(&args).unwrap_err();
            assert_ne!(err.code, RefusalCode::Io, "{}", err.message);

            let mut round_argv = vec!["round", "absent-ws", "--sim"];
            round_argv.extend(bad.iter().copied());
            let round_args =
                parse(&round_argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            let round_err = round::cmd_round(&round_args).unwrap_err();
            assert_ne!(round_err.code, RefusalCode::Io, "{}", round_err.message);
            assert!(
                !round_err.message.contains("unknown option"),
                "round must recognize solver/integrator options: {}",
                round_err.message
            );
        }

        // `ftx round` without `--sim`/`--stills` must refuse solver/integrator flags explicitly.
        let round_no_sim =
            parse(&["round", "absent-ws", "--newton-strategy", "adaptive-ilu"].map(String::from))
                .unwrap();
        let no_sim_err = round::cmd_round(&round_no_sim).unwrap_err();
        assert_eq!(no_sim_err.code, RefusalCode::InvalidDocument);
        assert!(
            no_sim_err
                .message
                .contains("--newton-strategy requires --sim or --stills"),
            "{}",
            no_sim_err.message
        );
    }
}
