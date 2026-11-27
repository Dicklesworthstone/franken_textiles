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
mod knit;
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
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use ftx_body::{
    AnnyModel, BodyFrames, FREE, HandCollider, Legs, Pose, PosedBody, RestBody, SkinMesh,
    SkinningMode, TAPE_FIELDS, TapeFit, ThumbPose, Wearer, clothing_skin_with_hands,
    default_model_path, fit_tape, pose_deltas, pose_deltas_with_thumbs, skin_mesh, tape_targets,
};
use ftx_core::Vec3;
use ftx_core::receipt::sha256_hex;
use ftx_core::{Refusal, RefusalCode};
use ftx_drape::animation::{mesh_map, pc2, seam_colours, strain_colours};
use ftx_drape::collider::{SurfaceAudit, audit_surface};
use ftx_drape::run::{DrapeBody, DrapeOptions, StaticEquilibriumOptions, drape_observed, viewer};
use ftx_drape::stepping::SubdivisionOptions;
use ftx_drape::{NewtonStrategy, TimeIntegrator};
use ftx_pattern::{Measurements, Pattern, alter, format_table, validate};
use serde_json::{Value, json};

const VIEWER_TEMPLATE: &str = include_str!("../../../assets/fold_viewer_template.html");

const USAGE: &str = "usage:
  ftx import garmentcode INPUT.json --out NEW_DIR [--annotations ANNOTATIONS.json] [--json]
  ftx draft zouave_bolero|zouave_trousers|fez --measurements M --out DIR [--style S.json] [--fabric F.json | --material-bindings B.json] [--json]
  ftx draft PROGRAM --measurements M --ws WS [--style S.json] [--fabric F.json | --material-bindings B.json] [--update-measurements] [--as-of DATE] [--no-priors] [--json]
            [--model PATH] [--body-fit REPORT] (draft from a tape-fitted or accepted body; MODEL_FIT, not new measurements)
  ftx validate PATTERN --measurements M [--fit F] [--model PATH] [--body-fit REPORT] [--flat-body] [--json]
  ftx round WS [--pattern P] [--measurements M] [--fit F] [--fabric F.json | --material-bindings B.json] [--export]
            [--sim|--stills] [--model PATH] [--body-fit REPORT] [--flat-body] [--spacing-mm MM] [--compression F] [--arm-deg DEG] [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm|knuckle|full] [--max-settle-steps N] [--res WIDTHxHEIGHT] [--shading standard|legible|both]
            [--thumb-pose JSON] [--static-equilibrium] [--static-tol-mm 0.001] [--static-max-newton 300]
            [--left-arm-deg DEG --right-arm-deg DEG] (independent initial arms; overrides --arm-deg)
            [--time-step-s SECONDS] [--sew N] [--drape N] [--elastic-ea N] [--elastic-hold-steps N] [--time-subdivision] [--subdivision-max-halvings 8] [--subdivision-max-trials 4096]
            [--newton-strategy projected|adaptive-ilu|trust-region] [--newton-tol-m-s M/S] [--integrator implicit-euler|generalized-alpha] [--rho-inf 0.0]
            [--bending-model quadratic|discrete-shells] [--compression-model axis-aligned|principal] [--air-drag] [--air-cn F] [--air-ct F] [--air-rho F] [--air-velocity VX,VY,VZ] [--rayleigh-membrane-s S] [--rayleigh-bending-s S]
            [--adaptive-barrier] [--stick-slip-mus F --stick-slip-muk F [--stick-slip-v F]] [--self-stick-slip-mus F --self-stick-slip-muk F [--self-stick-slip-v F]]
            [--min-spacing-mm MM] [--cvt-iters N] [--max-turning-angle-rad RAD] [--gather-refinement F]
            [--construction-standard] [--hem-width-mm MM] [--hem-mass-mult F] [--hem-bending-mult F] [--seam-width-mm MM] [--seam-mass-mult F] [--seam-bending-mult F]
            [--start-self-repulsion] [--start-repulsion-mm MM] [--start-repulsion-stiffness F] [--start-repulsion-period N] [--start-repulsion-max-checks N]
            [--update-measurements] [--note TEXT] [--as-of DATE] [--created YYYY-MM-DDTHH:MM] [--json]
  ftx drape PATTERN --measurements M --out DIR [--fit F] [--fabric F.json | --material-bindings B.json] [--sew N] [--drape N] [--time-step-s SECONDS] [--elastic-ea N] [--elastic-hold-steps N]
            [--two-start-sew N] [--spread-vertex-mm 10] [--spread-hem-mm 6]
            [--spread-clearance-mm 3] [--spread-seam-pp 1.5]
            [--model PATH] [--body-fit REPORT] [--arm-deg DEG] [--spacing-mm MM] [--compression F] [--self-contact-mm MM] [--self-mu MU] [--json]
            [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm|knuckle|full] [--thumb-pose JSON] [--max-settle-steps N]
            [--left-arm-deg DEG --right-arm-deg DEG] (both in 0..=180 degrees from vertical)
            [--static-equilibrium] [--static-tol-mm 0.001] [--static-max-newton 300] (independent frictionless comparison)
            [--time-subdivision] [--subdivision-max-halvings 8] [--subdivision-max-trials 4096]
            [--newton-strategy projected|adaptive-ilu|trust-region] [--newton-tol-m-s M/S] [--integrator implicit-euler|generalized-alpha] [--rho-inf 0.0]
            [--bending-model quadratic|discrete-shells] [--compression-model axis-aligned|principal] [--air-drag] [--air-cn F] [--air-ct F] [--air-rho F] [--air-velocity VX,VY,VZ] [--rayleigh-membrane-s S] [--rayleigh-bending-s S]
            [--adaptive-barrier] [--stick-slip-mus F --stick-slip-muk F [--stick-slip-v F]] [--self-stick-slip-mus F --self-stick-slip-muk F [--self-stick-slip-v F]]
            [--min-spacing-mm MM] [--cvt-iters N] [--max-turning-angle-rad RAD] [--gather-refinement F]
            [--construction-standard] [--hem-width-mm MM] [--hem-mass-mult F] [--hem-bending-mult F] [--seam-width-mm MM] [--seam-mass-mult F] [--seam-bending-mult F]
            [--start-self-repulsion] [--start-repulsion-mm MM] [--start-repulsion-stiffness F] [--start-repulsion-period N] [--start-repulsion-max-checks N]
            [--res 960x540] (size of the pictures a refused drape leaves under DIR/refused)
  ftx drape OUTFIT.json --measurements M --out NEW_DIR --self-contact-mm MM [--model PATH] [--json]
  ftx qualify (--ws WS | --measurements M --out DIR) [--pattern P] [--fabric F.json | --material-bindings B.json] [--model PATH] [--body-fit REPORT] [--arm-deg DEG]
              [--left-arm-deg DEG --right-arm-deg DEG]
              [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm|knuckle|full] [--thumb-pose JSON]
              [--compression F] [--newton-strategy projected|adaptive-ilu|trust-region] [--integrator implicit-euler|generalized-alpha] [--rho-inf 0.0]
              [--bending-model quadratic|discrete-shells] [--compression-model axis-aligned|principal] [--air-drag] [--air-cn F] [--air-ct F] [--air-rho F] [--air-velocity VX,VY,VZ] [--rayleigh-membrane-s S] [--rayleigh-bending-s S]
              [--adaptive-barrier] [--stick-slip-mus F --stick-slip-muk F [--stick-slip-v F]]
              [--construction-standard] [--hem-width-mm MM] [--hem-mass-mult F] [--hem-bending-mult F] [--seam-width-mm MM] [--seam-mass-mult F] [--seam-bending-mult F] [--json]
  ftx body --measurements M --out DIR [--pose drape|rest|arms-up|seated|squat] [--skinning lbs|dqs] [--thumb-pose JSON] [--model PATH] [--json]
  ftx body --body-fit REPORT --out NEW_DIR [--measurements M] [--pose rest|drape|arms-up|seated|squat] [--skinning lbs|dqs] [--thumb-pose JSON] [--model PATH] [--json]
  ftx place PATTERN --measurements M --out NEW_DIR [--fabric F.json | --material-bindings B.json] [--elastic-ea N] [--model PATH] [--body-fit REPORT] [--spacing-mm MM] [--arm-deg DEG]
            [--left-arm-deg DEG --right-arm-deg DEG]
            [--min-spacing-mm MM] [--cvt-iters N] [--max-turning-angle-rad RAD] [--gather-refinement F]
            [--start-self-repulsion] [--start-repulsion-mm MM] [--start-repulsion-stiffness F] [--start-repulsion-period N] [--start-repulsion-max-checks N]
            [--legs standing|seated|squat] [--leg-fraction F] [--skinning lbs|dqs] [--hands wrist|palm|knuckle|full] [--thumb-pose JSON] [--res 960x540] [--shading standard|legible] [--json] (start-state stills and numbers; no drape)
  ftx render DRAPE_DIR [--res 960x540] [--shading standard|legible] [--json]
             (legible: a second set, still_<view>_legible.png, drawn for reading folds and edges; describes no fabric)
  ftx render DRAPE_DIR --compare REVISED_DIR --view VIEW --out NEW_DIR [--res 960x540] [--shading standard|legible] [--json]
  ftx inspect-pixel ROUND_DIR --view front|side|back|three_quarter|strain_front|strain_back --pixel X,Y [--json]
  ftx viewer SIMDIR [--out HTML] [--pattern P] [--json]
  ftx viewer SIMDIR --check-contact [--json]
  ftx report WS [--json]
  ftx sim-points SIMDIR --pattern P --measurements M [--json]
  ftx outfit --measurements M --sim DIR [--sim DIR ...] --out NEW_DIR [--hat P] [--seams] [--res 960x960] [--shading standard|legible] [--json]
  ftx alter slash-spread PATTERN --piece P --axis x|y --at MM --add MM --out OUT [--json]
  ftx alter move PATTERN --piece P --vertex V [--dx MM] [--dy MM] --out OUT [--json]
  ftx alter move-control PATTERN --piece P --edge E --control 0|1 [--dx MM] [--dy MM] --out OUT [--json]
  ftx alter true-seam PATTERN --seam S [--end start|end] --out OUT [--json]
  ftx alter cap-ease PATTERN --piece P --seams S1,S2 --target RATIO --out OUT [--json]
  ftx alter transfer-curve PATTERN --from SRC.json:INSTANCE/EDGE --to PIECE/EDGE --out OUT [--json]
  ftx materials [list|catalog [SEARCH]|show ID|freeze ID|compare ID ID] [--allow-estimates] [--json]
  ftx materials study PATTERN.json STUDY.json [--adopt KEY --out NEW-PATTERN.json] [--json]
  ftx materials study-on-body PATTERN.json STUDY.json BODY.json MODEL.ftxbody [--adopt KEY --out NEW-PATTERN.json] [--json]
  ftx materials scenario-study PATTERN.json STUDY.json SCENARIO.json MEASUREMENTS.json MODEL.ftxbody [--adopt KEY --out NEW-PATTERN.json] [--json]
  ftx materials inspect SELECTION.json [--json]
  ftx materials resolve PATTERN.json [--json]
  ftx materials bind PATTERN.json SELECTION.json NEW-PATTERN.json [--json]
  ftx materials bind-changes PATTERN.json CHANGES.json NEW-PATTERN.json [--json]
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
  ftx measure calib-sheet --paper letter|a4 --out DIR [--square-mm MM] [--marker-mm MM] [--dictionary NAME] [--json]
  ftx measure calibrate-camera (FILE | --images MANIFEST --sheet JSON [--detector-python PY]) [--corner-noise JSON] [--out FILE] [--json]
  ftx measure photo-scale --image PNG [--paper letter|a4|auto] [--sheet JSON [--detector-python PY]] [--lens FILE --lens-id ID --rotation-cw-deg DEG [--independent-calibration-evidence TEXT]] [--out FILE] [--json]
  ftx measure photo-scale --reference JSON [--lens FILE --lens-id ID --rotation-cw-deg DEG [--independent-calibration-evidence TEXT]] [--out FILE] [--json]
  ftx measure sheet-pose --reference JSON [--calibration JSON --lens-id ID] [--out FILE] [--json]
  ftx measure measure-photo --image PNG|--reference JSON --points JSON [--paper letter|a4|auto] [--sheet JSON [--detector-python PY]]
      [--lens FILE --lens-id ID --rotation-cw-deg DEG [--independent-calibration-evidence TEXT]]
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
  ftx knit swatch --out DIR [--machine ID] [--yarn ID] [--material-sha256 SHA256] [--needles N] [--courses-per-zone N] [--cams C1,C2,...] [--created YYYY-MM-DDTHH:MM] [--json]
  ftx knit draft --age-class infant|child|adult --out DIR [--style panel|tube] [--machine ID] [--qual SWATCH.json --material-sha256 SHA256] [--drape-receipt RECEIPT.json] [--has-drawstring] [--json]
  ftx knit verify PATTERN.knit.json [--machine ID] [--qual SWATCH.json --material-sha256 SHA256] [--json]
  ftx knit export PATTERN.knit.json --out DIR [--machine ID] [--qual SWATCH.json --material-sha256 SHA256] [--drape-receipt RECEIPT.json] [--created YYYY-MM-DDTHH:MM] [--json]
  ftx knit pullover SPEC.json --gauge GAUGE.json --out DIR [--machine hand-knit|ID] [--qual SWATCH.json --material-sha256 SHA256] [--drape-receipts RECEIPTS.json] [--json]
  ftx knit poincloux [--mu MU] [--json]
  ftx dye swatch --out DIR [--machine piezo-cmyk|dip-3bath] [--yarn ID] [--json]
  ftx dye optimize --out DIR [--machine piezo-cmyk|dip-3bath] [--age-class infant|child|adult|swatch] [--regime stripes|pooling|fiducial|inline|jacquard] [--circular] [--image PNG] [--courses N] [--wales N] [--cells N] [--max-iters N] [--dye-qual Q.json] [--json]
  ftx dye verify PROGRAM.json [--machine piezo-cmyk|dip-3bath] [--dye-qual Q.json] [--out DIR] [--json]
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
    let command = argv
        .iter()
        .find(|a| a.as_str() != "--json")
        .map(String::as_str);
    let (mut positional, mut options, mut values, mut json) =
        (Vec::new(), BTreeMap::new(), Vec::new(), false);
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            json = true;
        } else if let Some(name) = a.strip_prefix("--") {
            // The reference uses --sim both as a round switch and a repeated outfit path;
            // --seams is a switch here, but takes a value in alter cap-ease.
            if command == Some("outfit") && name == "sim" {
                let value = it.next().ok_or("--sim needs a directory")?;
                if value.starts_with("--") {
                    return Err("--sim needs a directory".into());
                }
                values.push(value.clone());
                options.insert(name.to_string(), value.clone());
                continue;
            }
            if name == "allow-estimates"
                || (name == "check-contact" && command == Some("viewer"))
                || name == "static-equilibrium"
                || name == "time-subdivision"
                || name == "air-drag"
                || name == "adaptive-barrier"
                || name == "construction-standard"
                || name == "start-self-repulsion"
                || measure::BOOLEAN_OPTIONS.contains(&name)
                || export::BOOLEAN_OPTIONS.contains(&name)
                || knit::BOOLEAN_OPTIONS.contains(&name)
                || round::BOOLEAN_OPTIONS.contains(&name)
                || trace::BOOLEAN_OPTIONS.contains(&name)
                || (name == "seams" && command == Some("outfit"))
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

pub(crate) fn read_regular_bounded(
    path: &std::path::Path,
    limit: u64,
    label: &str,
) -> Result<Vec<u8>, Refusal> {
    let io = |e| workspace::io_error(path, e);
    let not_regular = || {
        bad_input(format!(
            "{}: {label} must be a regular file",
            path.display()
        ))
    };
    let budget = || {
        Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "{}: {label} exceeds {limit} byte input budget",
                path.display()
            ),
        )
    };
    let meta = std::fs::metadata(path).map_err(io)?;
    if !meta.is_file() {
        return Err(not_regular());
    }
    if meta.len() > limit {
        return Err(budget());
    }
    let file = std::fs::File::open(path).map_err(io)?;
    let fd_meta = file.metadata().map_err(io)?;
    if !fd_meta.is_file() {
        return Err(not_regular());
    }
    if fd_meta.len() > limit {
        return Err(budget());
    }
    let mut bytes = Vec::new();
    file.take(limit.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(io)?;
    if bytes.len() as u64 > limit {
        return Err(budget());
    }
    Ok(bytes)
}

fn read_json(path: &str) -> Result<(Value, Vec<u8>), Refusal> {
    const MAX_JSON_BYTES: u64 = 64 * 1024 * 1024;
    let bytes = read_regular_bounded(std::path::Path::new(path), MAX_JSON_BYTES, "JSON input")?;
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

/// `--hands wrist|palm|knuckle|full`: how much of each hand the clothing collider keeps. The default is
/// the reference's collider, capped at the wrists; the others are native. `palm` and `knuckle` keep the
/// palms, and with `knuckle` the first finger joints too; `full` keeps the whole hand, for a sleeve that
/// covers the fingers.
pub(crate) fn parse_hands(args: &Args) -> Result<HandCollider, Refusal> {
    match args.options.get("hands").map(String::as_str) {
        None => Ok(HandCollider::Wrist),
        Some(value) => HandCollider::parse(value).ok_or_else(|| {
            bad_input(format!("--hands {value}: no such hand collider"))
                .repair("--hands wrist, palm, knuckle or full")
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
        &[
            "measurements",
            "out",
            "pose",
            "skinning",
            "thumb-pose",
            "model",
        ],
    )?;
    if args.positional.len() != 1 {
        return Err(bad_input("body takes named options, not positional files"));
    }
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
    let thumbs = parse_thumb_pose(args)?;
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
    let posed = pose_fitted_body(&model, &rest, &pose, skinning, thumbs)?;
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
    if let Some(thumbs) = thumbs {
        doc.insert("thumb_pose".into(), json!(thumbs));
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
    let doc_bytes = serde_json::to_string_pretty(&doc).map_err(|e| bad_input(e.to_string()))?;
    write("body.json", doc_bytes.as_bytes())?;
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
        "move-control" => &["out", "piece", "edge", "control", "dx", "dy"],
        "true-seam" => &["out", "seam", "end"],
        "cap-ease" => &["out", "piece", "seams", "target"],
        "transfer-curve" => &["out", "from", "to"],
        _ => {
            return Err(bad_input(format!("ftx alter: no op {op:?}")).repair(
                "ftx alter slash-spread|move|move-control|true-seam|cap-ease|transfer-curve PATTERN ...",
            ));
        }
    };
    known_options(args, "alter", allowed)?;
    if args.positional.len() != 3 {
        return Err(bad_input(format!(
            "ftx alter {op} needs exactly one pattern file"
        )));
    }
    let pattern_path = &args.positional[2];
    let (doc, bytes) = fold::pattern_document(std::path::Path::new(pattern_path))?;
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
        "move-control" => alter::move_control(
            &doc,
            text("piece")?,
            text("edge")?,
            text("control")?
                .parse()
                .map_err(|_| bad_input("--control must be a zero-based Bézier control index"))?,
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
            let (src, _) = fold::pattern_document(std::path::Path::new(src_path))?;
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
    if !out.as_os_str().is_empty()
        && !round::has_trailing_dir_syntax(&out)
        && out.file_name().is_some()
        && let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty())
    {
        make_dir(parent)?;
    }
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
    if args.positional.len() != 2 {
        return Err(bad_input("ftx validate needs exactly one pattern file"));
    }
    let pattern_path = &args.positional[1];
    let (pat, _) = fold::pattern_document(std::path::Path::new(pattern_path))?;
    let (meas, _) = read_json(required(args, "measurements")?)?;
    let spec = fit_spec(args)?;
    let selected = ["model", "body-fit", "flat-body"]
        .iter()
        .any(|key| args.options.contains_key(*key));
    let meas = current_measurements(meas)?;
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
    if let Some(bm) = args.options.get("bending-model") {
        opts.bending_model = match bm.as_str() {
            "quadratic" => ftx_drape::BendingModel::Quadratic,
            "discrete-shells" => ftx_drape::BendingModel::DiscreteShells,
            _ => {
                return Err(bad_input(
                    "--bending-model must be quadratic or discrete-shells",
                ));
            }
        };
    }
    if let Some(cm) = args.options.get("compression-model") {
        opts.compression_model = match cm.as_str() {
            "axis-aligned" => ftx_drape::CompressionModel::AxisAligned,
            "principal" => ftx_drape::CompressionModel::Principal,
            _ => {
                return Err(bad_input(
                    "--compression-model must be axis-aligned or principal",
                ));
            }
        };
    }
    if args.options.contains_key("air-drag") {
        let mut drag = ftx_drape::AirDrag::standard();
        if let Some(v) = args.options.get("air-cn") {
            drag.c_normal = v
                .parse()
                .map_err(|_| bad_input("--air-cn must be a finite nonnegative drag coefficient"))?;
        }
        if let Some(v) = args.options.get("air-ct") {
            drag.c_tangent = v
                .parse()
                .map_err(|_| bad_input("--air-ct must be a finite nonnegative drag coefficient"))?;
        }
        if let Some(v) = args.options.get("air-rho") {
            drag.rho_air_kg_m3 = v.parse().map_err(|_| {
                bad_input("--air-rho must be a finite nonnegative air density in kg/m³")
            })?;
        }
        if let Some(v) = args.options.get("air-velocity") {
            let parts: Vec<&str> = v.split(',').collect();
            if parts.len() != 3 {
                return Err(bad_input(
                    "--air-velocity must be three comma-separated finite numbers VX,VY,VZ in m/s",
                ));
            }
            let mut wind = [0.0; 3];
            for (slot, raw) in wind.iter_mut().zip(parts) {
                *slot = raw.trim().parse().map_err(|_| {
                    bad_input(
                        "--air-velocity must be three comma-separated finite numbers VX,VY,VZ in m/s",
                    )
                })?;
            }
            drag.air_velocity_m_s = wind;
        }
        opts.damping.air_drag = Some(drag);
    } else if let Some(name) = ["air-cn", "air-ct", "air-rho", "air-velocity"]
        .into_iter()
        .find(|key| args.options.contains_key(*key))
    {
        return Err(bad_input(format!("--{name} requires --air-drag")));
    }
    if let Some(v) = args.options.get("rayleigh-membrane-s") {
        opts.damping.rayleigh_membrane_s = v.parse().map_err(|_| {
            bad_input("--rayleigh-membrane-s must be a finite nonnegative duration in seconds")
        })?;
    }
    if let Some(v) = args.options.get("rayleigh-bending-s") {
        opts.damping.rayleigh_bending_s = v.parse().map_err(|_| {
            bad_input("--rayleigh-bending-s must be a finite nonnegative duration in seconds")
        })?;
    }
    if args.options.contains_key("adaptive-barrier") {
        opts.adaptive_barrier = Some(ftx_drape::AdaptiveBarrierPolicy::default());
    }
    for (prefix, slot) in [
        ("stick-slip", &mut opts.stick_slip),
        ("self-stick-slip", &mut opts.self_stick_slip),
    ] {
        let mus_k = format!("{prefix}-mus");
        let muk_k = format!("{prefix}-muk");
        let v_k = format!("{prefix}-v");
        let any = args.options.contains_key(&mus_k)
            || args.options.contains_key(&muk_k)
            || args.options.contains_key(&v_k);
        if any {
            let mu_s = args
                .options
                .get(&mus_k)
                .ok_or_else(|| bad_input(format!("--{prefix}-* requires --{mus_k} and --{muk_k}")))?
                .parse::<f64>()
                .map_err(|_| {
                    bad_input(format!("--{mus_k} must be a finite friction coefficient"))
                })?;
            let mu_k = args
                .options
                .get(&muk_k)
                .ok_or_else(|| bad_input(format!("--{prefix}-* requires --{mus_k} and --{muk_k}")))?
                .parse::<f64>()
                .map_err(|_| {
                    bad_input(format!("--{muk_k} must be a finite friction coefficient"))
                })?;
            let transition_v_m_s = match args.options.get(&v_k) {
                Some(v) => v.parse::<f64>().map_err(|_| {
                    bad_input(format!(
                        "--{v_k} must be a positive transition speed in m/s"
                    ))
                })?,
                None => 0.02,
            };
            *slot = Some(ftx_drape::StickSlipFriction::new(
                mu_s,
                mu_k,
                transition_v_m_s,
            )?);
        }
    }
    if let Some(v) = args.options.get("min-spacing-mm") {
        opts.mesh.min_spacing_mm = Some(v.parse().map_err(|_| {
            bad_input("--min-spacing-mm must be a positive finite distance in millimetres")
        })?);
    }
    if let Some(v) = args.options.get("cvt-iters") {
        opts.mesh.cvt_iterations = v
            .parse()
            .map_err(|_| bad_input("--cvt-iters must be a non-negative integer"))?;
    }
    if let Some(v) = args.options.get("max-turning-angle-rad") {
        opts.mesh.max_turning_angle_rad = v
            .parse()
            .map_err(|_| bad_input("--max-turning-angle-rad must be a finite angle in radians"))?;
    }
    if let Some(v) = args.options.get("gather-refinement") {
        opts.mesh.gather_refinement = v
            .parse()
            .map_err(|_| bad_input("--gather-refinement must be a finite factor >= 1.0"))?;
    }
    if args.options.contains_key("construction-standard") {
        opts.construction = ftx_drape::ConstructionMechanicsOptions::standard();
    }
    for (name, slot, desc) in [
        (
            "hem-width-mm",
            &mut opts.construction.hem_width_mm,
            "a finite non-negative hem width in millimetres",
        ),
        (
            "hem-mass-mult",
            &mut opts.construction.hem_mass_multiplier,
            "a finite multiplier >= 1.0",
        ),
        (
            "hem-bending-mult",
            &mut opts.construction.hem_bending_multiplier,
            "a finite multiplier >= 1.0",
        ),
        (
            "seam-width-mm",
            &mut opts.construction.seam_width_mm,
            "a finite non-negative seam-allowance width in millimetres",
        ),
        (
            "seam-mass-mult",
            &mut opts.construction.seam_mass_multiplier,
            "a finite multiplier >= 1.0",
        ),
        (
            "seam-bending-mult",
            &mut opts.construction.seam_bending_multiplier,
            "a finite multiplier >= 1.0",
        ),
    ] {
        if let Some(v) = args.options.get(name) {
            *slot = v
                .parse()
                .map_err(|_| bad_input(format!("--{name} must be {desc}")))?;
        }
    }
    if args.options.contains_key("start-self-repulsion") {
        let mut rep = ftx_drape::SelfRepulsionOptions::default();
        if let Some(v) = args.options.get("start-repulsion-mm") {
            rep.min_separation_m = v.parse::<f64>().map_err(|_| {
                bad_input("--start-repulsion-mm must be a positive finite distance in millimetres")
            })? * 1e-3;
        }
        if let Some(v) = args.options.get("start-repulsion-stiffness") {
            rep.stiffness = v.parse().map_err(|_| {
                bad_input("--start-repulsion-stiffness must be a finite factor in (0, 1]")
            })?;
        }
        if let Some(v) = args.options.get("start-repulsion-period") {
            rep.period = v
                .parse()
                .map_err(|_| bad_input("--start-repulsion-period must be a positive integer"))?;
        }
        if let Some(v) = args.options.get("start-repulsion-max-checks") {
            rep.max_candidate_checks = v.parse().map_err(|_| {
                bad_input("--start-repulsion-max-checks must be a positive integer")
            })?;
        }
        opts.start_self_repulsion = Some(rep);
    } else if let Some(name) = [
        "start-repulsion-mm",
        "start-repulsion-stiffness",
        "start-repulsion-period",
        "start-repulsion-max-checks",
    ]
    .into_iter()
    .find(|key| args.options.contains_key(*key))
    {
        return Err(bad_input(format!(
            "--{name} requires --start-self-repulsion"
        )));
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
    match (
        args.options.get("left-arm-deg"),
        args.options.get("right-arm-deg"),
    ) {
        (None, None) => {}
        (Some(left), Some(right)) => {
            let angle = |value: &str, flag: &str| -> Result<f64, Refusal> {
                let angle = value.parse::<f64>().map_err(|_| {
                    bad_input(format!("--{flag} must be a finite angle in degrees"))
                })?;
                Refusal::finite(&format!("--{flag}"), angle)?;
                if !(0.0..=180.0).contains(&angle) {
                    return Err(Refusal::new(
                        RefusalCode::OutOfRange,
                        format!("--{flag} must be in 0..=180 degrees from vertical"),
                    ));
                }
                Ok(angle)
            };
            pose.arm_pose = Some(ftx_body::drape::ArmPose {
                left_abduction_deg: angle(left, "left-arm-deg")?,
                right_abduction_deg: angle(right, "right-arm-deg")?,
            });
        }
        _ => {
            return Err(bad_input(
                "--left-arm-deg and --right-arm-deg must be supplied together",
            ));
        }
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

pub(crate) fn parse_thumb_pose(args: &Args) -> Result<Option<ThumbPose>, Refusal> {
    let Some(value) = args.options.get("thumb-pose") else {
        return Ok(None);
    };
    let thumbs: ThumbPose =
        serde_json::from_str(value).map_err(|err| bad_input(format!("--thumb-pose: {err}")))?;
    thumbs.validate()?;
    Ok(Some(thumbs))
}

pub(crate) fn pose_fitted_body(
    model: &AnnyModel,
    rest: &RestBody,
    pose: &Pose,
    skinning: SkinningMode,
    thumbs: Option<ThumbPose>,
) -> Result<PosedBody, Refusal> {
    let deltas = match thumbs {
        Some(thumbs) => pose_deltas_with_thumbs(model, rest, pose, 1.0, &thumbs),
        None => pose_deltas(model, rest, pose, 1.0),
    }?;
    model.pose_with_skinning(rest, &deltas, skinning)
}

fn audit_authored_hands(
    model: &AnnyModel,
    posed: &PosedBody,
    skin: &SkinMesh,
    hands: HandCollider,
    thumbs: Option<ThumbPose>,
) -> Option<Result<SurfaceAudit, Refusal>> {
    thumbs.filter(|pose| *pose != ThumbPose::default())?;
    let audit =
        |skin: &SkinMesh| audit_surface(&skin.vertices, &skin.triangles, place::BODY_AUDIT_PAIRS);
    // Audit every displayed hand vertex, keeping the standard pelvic and foot caps of the
    // clothing collider. Cloth still receives its separate audit against the exact display mesh.
    Some(if hands == HandCollider::Full {
        audit(skin)
    } else {
        clothing_skin_with_hands(model, &posed.vertices, HandCollider::Full)
            .and_then(|full_hands| audit(&full_hands))
    })
}

/// Claim a fresh destination before fitting/solving. A failed retry must not leave a previous
/// successful run at the path the caller will inspect for its new result.
fn reserve_drape_output(out: &std::path::Path) -> Result<(), Refusal> {
    reserve_output(out, "drape")
}

fn reserve_output(out: &std::path::Path, command: &str) -> Result<(), Refusal> {
    match round::symlink_metadata(out) {
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
            "left-arm-deg",
            "right-arm-deg",
            "thumb-pose",
            "legs",
            "leg-fraction",
            "skinning",
            "hands",
            "body-fit",
            "two-start-sew",
            "fabric",
            "material-bindings",
            "spread-vertex-mm",
            "spread-hem-mm",
            "spread-clearance-mm",
            "spread-seam-pp",
            "res",
        ],
    )?;
    let opts = drape_options(args)?;
    let pair = two_start::options(args, &opts)?;
    let pose = drape_pose(args)?;
    let thumbs = parse_thumb_pose(args)?;
    let skinning = parse_skinning(args)?;
    let hands = parse_hands(args)?;
    // A drape writes no stills of its own. This is the size of the pictures a refusal leaves, which a
    // round sets to the size of its stills so a pixel means the same place in both.
    let refusal_size = render::resolution(args.options.get("res").map(String::as_str))?;
    if args.positional.len() != 2 {
        return Err(bad_input("ftx drape needs exactly one pattern file"));
    }
    let pattern_path = &args.positional[1];
    let (mut pat, pattern_bytes) = fold::pattern_document(std::path::Path::new(pattern_path))?;
    let outfit = if pat["schema"] == ftx_drape::outfit::SCHEMA {
        if args.options.contains_key("fabric")
            || args.options.contains_key("material-bindings")
            || pair.is_some()
        {
            return Err(bad_input(
                "joint outfit requests do not accept --fabric, --material-bindings or --two-start-sew; retain explicit source selections and one joint start",
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
    let fabric_override = fabric::load(args)?
        .map(|input| input.apply(&mut pat))
        .transpose()?;
    let snapshot = fabric_override
        .as_ref()
        .map(|_| fabric::snapshot_bytes(&pat, "effective drape pattern"))
        .transpose()?;
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
    let posed = pose_fitted_body(&model, &rest, &pose, skinning, thumbs)?;
    let skin = clothing_skin_with_hands(&model, &posed.vertices, hands)?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let authored_hand_surface = audit_authored_hands(&model, &posed, &skin, hands, thumbs)
        .transpose()
        .map_err(|refusal| {
            refusal.repair("Check the full-hand collider with ftx place, then revise --thumb-pose.")
        })?;
    let thumb_audit = authored_hand_surface.map(Ok);
    let frames = BodyFrames::new(&model, &posed)?;
    let posed_frames = ftx_drape::scene::PosedFrames::new(&frames, pose.arm_pose);
    let pattern = Pattern::new(pat);
    let body = DrapeBody {
        frames: &posed_frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(Pose::A_POSE_DEG),
        arm_pose: pose.arm_pose,
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
            refusal_size,
        ),
        // No solver state: the start itself was refused. Draw it from what the solver was given.
        None if refusal.code == RefusalCode::IntersectingStart => place::start_stills(
            refusal,
            &place::Staging {
                pattern: &pattern,
                label: pattern_path,
                spacing: material.options.spacing,
                mesh: material.options.mesh,
                start_self_repulsion: material.options.start_self_repulsion,
                elastic_ea: material.options.elastic_ea,
                frames: &posed_frames,
                collision: (&skin.vertices, &skin.triangles),
                display: (&full.vertices, &full.triangles),
                skinning,
                hands,
                arm_pose: pose.arm_pose,
                thumbs,
                thumb_audit: thumb_audit.as_ref(),
                fabric_override: None,
            },
            &out,
            refusal_size,
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
            json!(hands.drape_pose_line_with_arms(body.arm_deg, body.arm_pose)),
        );
        if let Some(arms) = pose.arm_pose {
            b.insert("arm_pose".into(), json!(arms));
        }
        if !pose.legs.is_standing() {
            b.insert("legs".into(), json!(pose.legs));
        }
        if pose.leg_fraction != 1.0 {
            b.insert("leg_fraction".into(), json!(pose.leg_fraction));
        }
        if skinning != SkinningMode::LinearBlend {
            b.insert("skinning".into(), json!(skinning.as_str()));
        }
        if let Some(thumbs) = thumbs {
            b.insert("thumb_pose".into(), json!(thumbs));
        }
        if let Some(audit) = authored_hand_surface {
            b.insert(
                "authored_hand_surface_pairs_tested".into(),
                json!(audit.pairs_tested),
            );
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
    if let Some((selected, snapshot)) = fabric_override.as_ref().zip(snapshot) {
        files.extend(selected.entries()?);
        report["effective_pattern_sha256"] = json!(sha256_hex(&snapshot));
        files.push(("effective_pattern.json", snapshot));
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
    if !hands.is_wrist() {
        text.push_str(&format!("hand collider: {}\n", hands.as_str()));
    }
    if let Some(arms) = pose.arm_pose {
        text.push_str(&format!(
            "arm abduction: L {}°, R {}°\n",
            ftx_export::text::fnum(arms.left_abduction_deg, 2),
            ftx_export::text::fnum(arms.right_abduction_deg, 2),
        ));
    }
    append_drape_constitutive_summary(&report, Some(&mut text), None);
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
    if let Some(hand_collider) = report.get("hand_collider") {
        result["hand_collider"] = hand_collider.clone();
    }
    if let Some(thumbs) = thumbs {
        result["thumb_pose"] = json!(thumbs);
    }
    if let Some(arms) = pose.arm_pose {
        result["arm_pose"] = json!(arms);
    }
    if let Some(override_receipt) = report.get("fabric_override") {
        result["fabric_override"] = override_receipt.clone();
    }
    if let Some(hash) = report.get("effective_pattern_sha256") {
        result["effective_pattern_sha256"] = hash.clone();
    }
    append_drape_constitutive_summary(&report, None, Some(&mut result));
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

struct ElasticHoldCasing {
    instance: String,
    signed_mm: f64,
    max_abs_mm: f64,
    max_speed_mm_s: f64,
    quiet_at_1_mm_s: Option<bool>,
}

fn elastic_hold_casings(report: &Value) -> Vec<ElasticHoldCasing> {
    report["elastic_observation_windows"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|entry| {
            let instance = entry.get("instance")?.as_str().filter(|s| !s.is_empty())?;
            let hold = entry.get("declared_hold")?.as_object()?;
            if hold.get("status").and_then(Value::as_str) != Some("available") {
                return None;
            }
            let signed_mm = hold
                .get("signed_vertical_displacement_m")?
                .as_f64()
                .map(|v| v * 1e3)
                .filter(|v| v.is_finite())?;
            let max_abs_mm = hold
                .get("maximum_absolute_vertical_displacement_m")?
                .as_f64()
                .map(|v| v * 1e3)
                .filter(|v| v.is_finite())?;
            let max_speed_mm_s = hold
                .get("maximum_vertex_speed_m_s")?
                .as_f64()
                .map(|v| v * 1e3)
                .filter(|v| v.is_finite())?;
            let quiet_at_1_mm_s = hold.get("quiet_at_1_mm_s").and_then(Value::as_bool);
            Some(ElasticHoldCasing {
                instance: instance.to_string(),
                signed_mm,
                max_abs_mm,
                max_speed_mm_s,
                quiet_at_1_mm_s,
            })
        })
        .collect()
}

fn format_elastic_hold_casings(casings: &[ElasticHoldCasing]) -> String {
    casings
        .iter()
        .map(|c| {
            let quiet = match c.quiet_at_1_mm_s {
                Some(true) => ", quiet ≤1 mm/s",
                Some(false) => ", moving >1 mm/s",
                None => "",
            };
            format!(
                "{}: Δz {} mm, max |Δz| {} mm, max speed {} mm/s{quiet}",
                c.instance,
                ftx_export::text::fnum(c.signed_mm, 2),
                ftx_export::text::fnum(c.max_abs_mm, 2),
                ftx_export::text::fnum(c.max_speed_mm_s, 2),
            )
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn append_drape_constitutive_summary(
    report: &Value,
    text: Option<&mut String>,
    result: Option<&mut Value>,
) {
    let scheme = report["membrane_history"]["integration"]
        .as_str()
        .filter(|s| *s != "backward_euler");
    let bending_model = report["bending_history"]["model"].as_str();
    let bending_j = report["bending_history"]["plastic_dissipation_j"]
        .as_f64()
        .filter(|v| v.is_finite());
    let membrane_model = report["membrane_plasticity_history"]["model"].as_str();
    let membrane_j = report["membrane_plasticity_history"]["plastic_dissipation_j"]
        .as_f64()
        .filter(|v| v.is_finite());
    let damage_model = report["membrane_plasticity_history"]["damage_model"].as_str();
    let damage_j = report["membrane_plasticity_history"]["damage_dissipation_j"]
        .as_f64()
        .filter(|v| v.is_finite());
    let time_sub = report["numerical"]["time_subdivision"]
        .as_object()
        .and_then(|sub| {
            Some((
                sub.get("subdivided_nominal_intervals")?.as_u64()?,
                sub.get("nominal_intervals")?.as_u64()?,
                sub.get("accepted_substeps")?.as_u64()?,
                sub.get("trials")?.as_u64()?,
            ))
        });
    let start_repair = report.get("start_repair").filter(|v| v.is_object());
    let elastic_hold = report["elastic_hold_window"].as_object().and_then(|hold| {
        let steps = hold.get("requested_steps")?.as_u64().filter(|&n| n > 0)?;
        let status = hold
            .get("status")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("unknown");
        Some((steps, status))
    });
    let hold_casings = if elastic_hold.is_some() {
        elastic_hold_casings(report)
    } else {
        Vec::new()
    };
    if let Some(text) = text {
        if let Some((subdivided, nominal, accepted, trials)) = time_sub {
            text.push_str(&format!(
                "time subdivision: {subdivided}/{nominal} intervals subdivided ({accepted} substeps, {trials} trials)\n"
            ));
        }
        if let Some(repair) = start_repair {
            let mut parts = Vec::new();
            if let (Some(rej), Some(acc)) = (
                repair.get("rejected_relax_iters").and_then(Value::as_u64),
                repair.get("accepted_relax_iters").and_then(Value::as_u64),
            ) {
                parts.push(format!("earlier checkpoint ({rej} → {acc} iters)"));
            }
            if let Some(moved) = repair.get("moved_welded_vertices").and_then(Value::as_u64) {
                let max_note = repair
                    .get("maximum_displacement_mm")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .map(|mm| format!(", max {} mm", ftx_export::text::fnum(mm, 2)))
                    .unwrap_or_default();
                parts.push(format!(
                    "opposing leg separation ({moved} vertices{max_note})"
                ));
            }
            if !parts.is_empty() {
                text.push_str(&format!("start repair: {}\n", parts.join("; ")));
            } else if let Some(basis) = repair
                .get("basis")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                text.push_str(&format!("start repair: {basis}\n"));
            }
        }
        if let Some((steps, status)) = elastic_hold {
            if hold_casings.is_empty() {
                text.push_str(&format!(
                    "elastic hold window: {steps} step(s) ({status})\n"
                ));
            } else {
                text.push_str(&format!(
                    "elastic hold window: {steps} step(s) ({status}) — {}\n",
                    format_elastic_hold_casings(&hold_casings)
                ));
            }
        }
        if let Some(scheme) = scheme {
            text.push_str(&format!("relaxation integration: {scheme}\n"));
        }
        if let Some(model) = bending_model {
            match bending_j {
                Some(j) => text.push_str(&format!("bending plasticity: {model} ({j:.3e} J)\n")),
                None => text.push_str(&format!("bending plasticity: {model}\n")),
            }
        }
        if let Some(model) = membrane_model {
            match membrane_j {
                Some(j) => text.push_str(&format!("membrane plasticity: {model} ({j:.3e} J)\n")),
                None => text.push_str(&format!("membrane plasticity: {model}\n")),
            }
        }
        if let Some(model) = damage_model {
            match damage_j {
                Some(j) => text.push_str(&format!("membrane damage: {model} ({j:.3e} J)\n")),
                None => text.push_str(&format!("membrane damage: {model}\n")),
            }
        }
    }
    if let Some(out) = result {
        if let Some(repair) = start_repair {
            out["start_repair"] = repair.clone();
        }
        if let Some((steps, status)) = elastic_hold {
            let mut block = json!({"requested_steps": steps, "status": status});
            if !hold_casings.is_empty() {
                block["casings"] = Value::Array(
                    hold_casings
                        .iter()
                        .map(|c| {
                            let mut row = json!({
                                "instance": c.instance,
                                "signed_vertical_displacement_mm": c.signed_mm,
                                "maximum_absolute_vertical_displacement_mm": c.max_abs_mm,
                                "maximum_vertex_speed_mm_s": c.max_speed_mm_s,
                            });
                            if let Some(quiet) = c.quiet_at_1_mm_s {
                                row["quiet_at_1_mm_s"] = json!(quiet);
                            }
                            row
                        })
                        .collect(),
                );
            }
            out["elastic_hold_window"] = block;
        }
        if let Some(scheme) = scheme {
            out["relaxation_integration"] = json!(scheme);
        }
        if let Some(model) = bending_model {
            let mut bp = json!({"model": model});
            if let Some(j) = bending_j {
                bp["plastic_dissipation_j"] = json!(j);
            }
            out["bending_plasticity"] = bp;
        }
        if let Some(model) = membrane_model {
            let mut mp = json!({"model": model});
            if let Some(j) = membrane_j {
                mp["plastic_dissipation_j"] = json!(j);
            }
            out["membrane_plasticity"] = mp;
        }
        if let Some(model) = damage_model {
            let mut dmg = json!({"damage_model": model});
            if let Some(j) = damage_j {
                dmg["damage_dissipation_j"] = json!(j);
            }
            out["membrane_damage"] = dmg;
        }
    }
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
                argv.iter()
                    .find(|a| a.as_str() != "--json")
                    .map_or("ftx", String::as_str),
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
        Some("knit") => emit("knit", args.json, knit::cmd_knit(&args)),
        Some("dye") => emit("dye", args.json, knit::cmd_dye(&args)),
        Some("trace") => emit("trace", args.json, trace::cmd_trace(&args)),
        Some("annotate") => emit("annotate", args.json, trace::cmd_annotate(&args)),
        Some("scale-fit") => emit("scale-fit", args.json, trace::cmd_scale_fit(&args)),
        Some("audit") => emit("audit", args.json, trace::cmd_audit(&args)),
        Some("check") => emit(
            "check",
            args.json,
            export::cmd_check(&args, args.positional.get(1..).unwrap_or(&[])),
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
    fn independent_arm_angles_override_the_common_angle_and_require_both_sides() {
        let args = parse(
            &[
                "drape",
                "--arm-deg",
                "45",
                "--left-arm-deg",
                "0",
                "--right-arm-deg",
                "180",
            ]
            .map(String::from),
        )
        .unwrap();
        let pose = drape_pose(&args).unwrap();
        assert_eq!(pose.arm_abduction_deg, Some(45.0));
        assert_eq!(
            pose.arm_pose,
            Some(ftx_body::drape::ArmPose {
                left_abduction_deg: 0.0,
                right_abduction_deg: 180.0,
            })
        );
        assert!(pose.straight_arms);
        for flags in [
            vec!["--left-arm-deg", "30"],
            vec!["--right-arm-deg", "70"],
            vec!["--left-arm-deg", "NaN", "--right-arm-deg", "70"],
            vec!["--left-arm-deg", "30", "--right-arm-deg", "inf"],
            vec!["--left-arm-deg", "-1", "--right-arm-deg", "70"],
            vec!["--left-arm-deg", "30", "--right-arm-deg", "181"],
        ] {
            let mut raw = vec!["drape", "absent.json"];
            raw.extend(flags);
            let args = parse(&raw.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            let refusal = cmd_drape(&args).unwrap_err();
            assert_ne!(refusal.code, RefusalCode::Io);
            assert!(refusal.message.contains("arm-deg"), "{refusal:?}");
        }
    }

    #[test]
    fn authored_thumb_pose_is_validated_before_reading_inputs() {
        let absent = parse(&["drape".into()]).unwrap();
        assert_eq!(parse_thumb_pose(&absent).unwrap(), None);
        let valid =
            r#"{"left_rotation_vector_rad":[0.2,0,-0.3],"right_rotation_vector_rad":[0.2,0,0.3]}"#;
        let args =
            parse(&["drape", "absent.json", "--thumb-pose", valid].map(String::from)).unwrap();
        let pose = parse_thumb_pose(&args).unwrap().unwrap();
        assert_eq!(pose.left_rotation_vector_rad, [0.2, 0.0, -0.3]);
        assert_eq!(pose.right_rotation_vector_rad, [0.2, 0.0, 0.3]);
        for input in [
            r#"{"left_rotation_vector_rad":[0,0,0]}"#,
            r#"{"left_rotation_vector_rad":[0,0],"right_rotation_vector_rad":[0,0,0]}"#,
            r#"{"left_rotation_vector_rad":[0,0,0],"right_rotation_vector_rad":[0,1e999,0]}"#,
        ] {
            let args =
                parse(&["drape", "absent.json", "--thumb-pose", input].map(String::from)).unwrap();
            let refusal = cmd_drape(&args).unwrap_err();
            assert!(refusal.message.contains("--thumb-pose"), "{refusal:?}");
            let qualify =
                parse(&["qualify", "no-such-ws", "--thumb-pose", input].map(String::from)).unwrap();
            let qualifying_refusal = round::cmd_qualify(&qualify).unwrap_err();
            assert_eq!(qualifying_refusal.code, refusal.code);
            assert_eq!(qualifying_refusal.message, refusal.message);
            for body_args in [
                vec![
                    "body",
                    "--measurements",
                    "absent.json",
                    "--out",
                    "absent",
                    "--thumb-pose",
                    input,
                ],
                vec![
                    "body",
                    "--body-fit",
                    "absent.json",
                    "--out",
                    "absent",
                    "--thumb-pose",
                    input,
                ],
            ] {
                let body_parsed =
                    parse(&body_args.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
                let body_refusal = cmd_body(&body_parsed).unwrap_err();
                assert_eq!(body_refusal.code, refusal.code);
                assert_eq!(body_refusal.message, refusal.message);
            }
        }
        let outside = r#"{"left_rotation_vector_rad":[0,0,0],"right_rotation_vector_rad":[2,2,2]}"#;
        let args =
            parse(&["drape", "absent.json", "--thumb-pose", outside].map(String::from)).unwrap();
        assert_eq!(cmd_drape(&args).unwrap_err().code, RefusalCode::OutOfRange);
        let body_outside = parse(
            &[
                "body",
                "--measurements",
                "absent.json",
                "--out",
                "absent",
                "--thumb-pose",
                outside,
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_body(&body_outside).unwrap_err().code,
            RefusalCode::OutOfRange
        );
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
    fn the_hand_collider_is_the_reference_wrist_cap_unless_another_is_asked_for() {
        let none = parse(&["drape".into()]).unwrap();
        assert_eq!(parse_hands(&none).unwrap(), HandCollider::Wrist);
        for (word, expected) in [
            ("wrist", HandCollider::Wrist),
            ("palm", HandCollider::Palm),
            ("knuckle", HandCollider::Knuckle),
            ("full", HandCollider::Full),
        ] {
            let args = parse(&["drape", "--hands", word].map(String::from)).unwrap();
            assert_eq!(parse_hands(&args).unwrap(), expected);
            assert_eq!(expected.as_str(), word);
        }
        // Any other word refuses before a pattern or a body is read, on each command that takes it.
        for command in ["drape", "place", "qualify"] {
            let args = parse(&[command, "no-pattern.json", "--hands", "mitten"].map(String::from))
                .unwrap();
            let refusal = match command {
                "drape" => cmd_drape(&args).unwrap_err(),
                "place" => place::run(&args).unwrap_err(),
                _ => round::cmd_qualify(&args).unwrap_err(),
            };
            assert_ne!(refusal.code, RefusalCode::Io, "{command}");
            assert!(
                refusal.message.contains("--hands mitten"),
                "{command}: {}",
                refusal.message
            );
            assert_eq!(refusal.repairs, ["--hands wrist, palm, knuckle or full"]);
        }
        let round = parse(&["round", "no-such-ws", "--sim", "--hands", "mitten"].map(String::from))
            .unwrap();
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
    fn drape_constitutive_summary_surfaces_opt_in_exact_exponential_and_damage_without_altering_defaults()
     {
        let mut default_text = String::new();
        let mut default_json = json!({});
        append_drape_constitutive_summary(
            &json!({
                "membrane_history": {"integration": "backward_euler"},
                "elastic_hold_window": {"requested_steps": 0, "status": "not_requested"}
            }),
            Some(&mut default_text),
            Some(&mut default_json),
        );
        assert!(default_text.is_empty());
        assert_eq!(default_json, json!({}));

        let mut opt_text = String::new();
        let mut opt_json = json!({});
        append_drape_constitutive_summary(
            &json!({
                "membrane_history": {"integration": "exact_exponential"},
                "membrane_plasticity_history": {
                    "damage_model": "isotropic_accumulated_damage/1",
                    "damage_dissipation_j": 0.00125
                }
            }),
            Some(&mut opt_text),
            Some(&mut opt_json),
        );
        assert!(opt_text.contains("relaxation integration: exact_exponential\n"));
        assert!(
            opt_text.contains("membrane damage: isotropic_accumulated_damage/1 (1.250e-3 J)\n")
        );
        assert_eq!(opt_json["relaxation_integration"], "exact_exponential");
        assert_eq!(
            opt_json["membrane_damage"]["damage_model"],
            "isotropic_accumulated_damage/1"
        );
        assert_eq!(opt_json["membrane_damage"]["damage_dissipation_j"], 0.00125);
        assert!(opt_json.get("bending_plasticity").is_none());
        assert!(opt_json.get("membrane_plasticity").is_none());

        let mut plastic_text = String::new();
        let mut plastic_json = json!({});
        append_drape_constitutive_summary(
            &json!({
                "bending_history": {
                    "model": "orthotropic_bending_plasticity/1",
                    "plastic_dissipation_j": 0.0025
                },
                "membrane_plasticity_history": {
                    "model": "hill48_membrane_plasticity/1",
                    "plastic_dissipation_j": 0.005
                }
            }),
            Some(&mut plastic_text),
            Some(&mut plastic_json),
        );
        assert!(
            plastic_text
                .contains("bending plasticity: orthotropic_bending_plasticity/1 (2.500e-3 J)\n")
        );
        assert!(
            plastic_text
                .contains("membrane plasticity: hill48_membrane_plasticity/1 (5.000e-3 J)\n")
        );
        assert_eq!(
            plastic_json["bending_plasticity"]["model"],
            "orthotropic_bending_plasticity/1"
        );
        assert_eq!(
            plastic_json["bending_plasticity"]["plastic_dissipation_j"],
            0.0025
        );
        assert_eq!(
            plastic_json["membrane_plasticity"]["model"],
            "hill48_membrane_plasticity/1"
        );
        assert_eq!(
            plastic_json["membrane_plasticity"]["plastic_dissipation_j"],
            0.005
        );

        let repair = json!({
            "basis": "earlier_contact_clear_relaxation_and_opposing_leg_placement",
            "rejected_relax_iters": 600,
            "accepted_relax_iters": 400,
            "moved_welded_vertices": 14,
            "maximum_displacement_mm": 3.25
        });
        let mut diag_text = String::new();
        let mut diag_json = json!({});
        append_drape_constitutive_summary(
            &json!({
                "numerical": {
                    "time_subdivision": {
                        "subdivided_nominal_intervals": 3,
                        "nominal_intervals": 12,
                        "accepted_substeps": 18,
                        "trials": 21
                    }
                },
                "start_repair": repair,
                "elastic_hold_window": {"requested_steps": 6, "status": "available"}
            }),
            Some(&mut diag_text),
            Some(&mut diag_json),
        );
        assert!(
            diag_text
                .contains("time subdivision: 3/12 intervals subdivided (18 substeps, 21 trials)\n")
        );
        assert!(diag_text.contains(
            "start repair: earlier checkpoint (600 → 400 iters); opposing leg separation (14 vertices, max 3.25 mm)\n"
        ));
        assert!(diag_text.contains("elastic hold window: 6 step(s) (available)\n"));
        assert_eq!(diag_json["start_repair"], repair);
        assert_eq!(
            diag_json["elastic_hold_window"],
            json!({"requested_steps": 6, "status": "available"})
        );

        let mut casing_text = String::new();
        let mut casing_json = json!({});
        append_drape_constitutive_summary(
            &json!({
                "elastic_hold_window": {"requested_steps": 6, "status": "available"},
                "elastic_observation_windows": [
                    {
                        "instance": "waistband",
                        "declared_hold": {
                            "status": "available",
                            "signed_vertical_displacement_m": -0.0004,
                            "maximum_absolute_vertical_displacement_m": 0.0005,
                            "maximum_vertex_speed_m_s": 0.0008,
                            "quiet_at_1_mm_s": true
                        }
                    },
                    {
                        "instance": "hem_cuff",
                        "declared_hold": {
                            "status": "available",
                            "signed_vertical_displacement_m": 0.0012,
                            "maximum_absolute_vertical_displacement_m": 0.0015,
                            "maximum_vertex_speed_m_s": 0.0024,
                            "quiet_at_1_mm_s": false
                        }
                    },
                    {
                        "instance": "ignored",
                        "declared_hold": {"status": "not_requested"}
                    }
                ]
            }),
            Some(&mut casing_text),
            Some(&mut casing_json),
        );
        assert_eq!(
            casing_text,
            "elastic hold window: 6 step(s) (available) — waistband: Δz -0.4 mm, max |Δz| 0.5 mm, max speed 0.8 mm/s, quiet ≤1 mm/s; hem_cuff: Δz 1.2 mm, max |Δz| 1.5 mm, max speed 2.4 mm/s, moving >1 mm/s\n"
        );
        assert_eq!(
            casing_json["elastic_hold_window"]["casings"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            casing_json["elastic_hold_window"]["casings"][0]["instance"],
            "waistband"
        );
        assert_eq!(
            casing_json["elastic_hold_window"]["casings"][0]["quiet_at_1_mm_s"],
            true
        );
        assert_eq!(
            casing_json["elastic_hold_window"]["casings"][1]["quiet_at_1_mm_s"],
            false
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

        // Leading --json must not disable command-scoped parsing for outfit/viewer flags.
        let outfit = parse(
            &[
                "--json",
                "outfit",
                "--measurements",
                "m.json",
                "--sim",
                "d1",
                "--sim",
                "d2",
                "--seams",
                "--out",
                "o",
            ]
            .map(String::from),
        )
        .unwrap();
        assert!(outfit.json);
        assert_eq!(outfit.positional, ["outfit"]);
        assert_eq!(outfit.values, ["d1", "d2"]);
        assert_eq!(outfit.options["seams"], "true");
        assert_eq!(outfit.options["out"], "o");

        let viewer =
            parse(&["--json", "viewer", "simdir", "--check-contact"].map(String::from)).unwrap();
        assert!(viewer.json);
        assert_eq!(viewer.positional, ["viewer", "simdir"]);
        assert_eq!(viewer.options["check-contact"], "true");
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
    fn a_drape_checks_the_size_of_its_refusal_pictures_before_reading_anything() {
        // The pattern file does not exist, so a refusal about the size shows it was checked first.
        for (res, code) in [
            ("0x32", RefusalCode::OutOfRange),
            ("840X620", RefusalCode::OutOfRange),
            ("8192x8192", RefusalCode::BudgetExceeded),
        ] {
            let args =
                parse(&["drape", "no-pattern.json", "--res", res].map(String::from)).unwrap();
            let refusal = cmd_drape(&args).unwrap_err();
            assert_eq!(refusal.code, code, "{res}: {}", refusal.message);
        }
        // An allowed size gets past that check and stops at the missing pattern instead.
        let args =
            parse(&["drape", "no-pattern.json", "--res", "840x620"].map(String::from)).unwrap();
        let refusal = cmd_drape(&args).unwrap_err();
        assert!(!refusal.message.contains("--res"), "{}", refusal.message);
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
                !round_err.message.contains("has no option")
                    && !round_err.message.contains("unknown option"),
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

    #[test]
    fn physics_realism_cli_options_parse_and_refuse_fail_closed() {
        let def = drape_options(&parse(&["drape".into()]).unwrap()).unwrap();
        assert!(def.bending_model.is_default());
        assert!(def.compression_model.is_default());
        assert!(def.damping.is_default());
        assert!(def.adaptive_barrier.is_none());
        assert!(def.stick_slip.is_none());
        assert!(def.self_stick_slip.is_none());

        let rich = drape_options(
            &parse(
                &[
                    "drape",
                    "--bending-model",
                    "discrete-shells",
                    "--compression-model",
                    "principal",
                    "--air-drag",
                    "--air-cn",
                    "1.4",
                    "--air-ct",
                    "0.08",
                    "--air-rho",
                    "1.2",
                    "--air-velocity",
                    "-2.5,1.25,0.5",
                    "--rayleigh-membrane-s",
                    "0.002",
                    "--rayleigh-bending-s",
                    "0.004",
                    "--adaptive-barrier",
                    "--self-contact-mm",
                    "0.5",
                    "--stick-slip-mus",
                    "0.6",
                    "--stick-slip-muk",
                    "0.3",
                    "--stick-slip-v",
                    "0.025",
                    "--self-stick-slip-mus",
                    "0.5",
                    "--self-stick-slip-muk",
                    "0.25",
                    "--min-spacing-mm",
                    "5.0",
                    "--cvt-iters",
                    "4",
                    "--max-turning-angle-rad",
                    "0.25",
                    "--gather-refinement",
                    "2.2",
                    "--construction-standard",
                    "--hem-width-mm",
                    "18.0",
                    "--seam-bending-mult",
                    "4.5",
                    "--start-self-repulsion",
                    "--start-repulsion-mm",
                    "3.5",
                    "--start-repulsion-stiffness",
                    "0.6",
                    "--start-repulsion-period",
                    "2",
                    "--start-repulsion-max-checks",
                    "1000000",
                ]
                .map(String::from),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(rich.bending_model, ftx_drape::BendingModel::DiscreteShells);
        assert_eq!(
            rich.compression_model,
            ftx_drape::CompressionModel::Principal
        );
        let drag = rich.damping.air_drag.unwrap();
        assert_eq!(drag.c_normal, 1.4);
        assert_eq!(drag.c_tangent, 0.08);
        assert_eq!(drag.rho_air_kg_m3, 1.2);
        assert_eq!(drag.air_velocity_m_s, [-2.5, 1.25, 0.5]);
        assert_eq!(rich.damping.rayleigh_membrane_s, 0.002);
        assert_eq!(rich.damping.rayleigh_bending_s, 0.004);
        assert_eq!(
            rich.adaptive_barrier,
            Some(ftx_drape::AdaptiveBarrierPolicy::default())
        );
        assert_eq!(
            rich.stick_slip,
            Some(ftx_drape::StickSlipFriction::new(0.6, 0.3, 0.025).unwrap())
        );
        assert_eq!(
            rich.self_stick_slip,
            Some(ftx_drape::StickSlipFriction::new(0.5, 0.25, 0.02).unwrap())
        );
        assert_eq!(rich.mesh.min_spacing_mm, Some(5.0));
        assert_eq!(rich.mesh.cvt_iterations, 4);
        assert_eq!(rich.mesh.max_turning_angle_rad, 0.25);
        assert_eq!(rich.mesh.gather_refinement, 2.2);
        let std_c = ftx_drape::ConstructionMechanicsOptions::standard();
        assert_eq!(rich.construction.hem_width_mm, 18.0);
        assert_eq!(
            rich.construction.hem_mass_multiplier,
            std_c.hem_mass_multiplier
        );
        assert_eq!(
            rich.construction.hem_bending_multiplier,
            std_c.hem_bending_multiplier
        );
        assert_eq!(rich.construction.seam_width_mm, std_c.seam_width_mm);
        assert_eq!(
            rich.construction.seam_mass_multiplier,
            std_c.seam_mass_multiplier
        );
        assert_eq!(rich.construction.seam_bending_multiplier, 4.5);
        let rep = rich.start_self_repulsion.unwrap();
        assert!((rep.min_separation_m - 0.0035).abs() < 1e-12);
        assert_eq!(rep.stiffness, 0.6);
        assert_eq!(rep.period, 2);
        assert_eq!(rep.max_candidate_checks, 1_000_000);

        // Non-physical command without --self-contact-mm must refuse --self-stick-slip-*.
        let no_sc = drape_options(
            &parse(
                &[
                    "place",
                    "--self-stick-slip-mus",
                    "0.5",
                    "--self-stick-slip-muk",
                    "0.25",
                ]
                .map(String::from),
            )
            .unwrap(),
        )
        .unwrap_err();
        assert_eq!(no_sc.code, RefusalCode::InvalidDocument);

        // Valid FTX-27d/e/f/g flags must pass known_options on both `drape` and `round --sim`.
        for cmd_argv in [
            vec![
                "drape",
                "absent-pattern.json",
                "--adaptive-barrier",
                "--self-contact-mm",
                "0.5",
                "--stick-slip-mus",
                "0.6",
                "--stick-slip-muk",
                "0.3",
                "--stick-slip-v",
                "0.025",
                "--self-stick-slip-mus",
                "0.5",
                "--self-stick-slip-muk",
                "0.25",
                "--self-stick-slip-v",
                "0.015",
                "--min-spacing-mm",
                "5.0",
                "--cvt-iters",
                "4",
                "--max-turning-angle-rad",
                "0.25",
                "--gather-refinement",
                "2.0",
                "--construction-standard",
                "--hem-width-mm",
                "16.0",
                "--hem-mass-mult",
                "2.2",
                "--hem-bending-mult",
                "4.0",
                "--seam-width-mm",
                "12.0",
                "--seam-mass-mult",
                "2.0",
                "--seam-bending-mult",
                "3.5",
                "--start-self-repulsion",
                "--start-repulsion-mm",
                "3.0",
                "--start-repulsion-stiffness",
                "0.5",
                "--start-repulsion-period",
                "2",
                "--start-repulsion-max-checks",
                "500000",
            ],
            vec![
                "round",
                "absent-ws",
                "--sim",
                "--adaptive-barrier",
                "--self-contact-mm",
                "0.5",
                "--stick-slip-mus",
                "0.6",
                "--stick-slip-muk",
                "0.3",
                "--stick-slip-v",
                "0.025",
                "--self-stick-slip-mus",
                "0.5",
                "--self-stick-slip-muk",
                "0.25",
                "--self-stick-slip-v",
                "0.015",
                "--min-spacing-mm",
                "5.0",
                "--cvt-iters",
                "4",
                "--max-turning-angle-rad",
                "0.25",
                "--gather-refinement",
                "2.0",
                "--construction-standard",
                "--hem-width-mm",
                "16.0",
                "--hem-mass-mult",
                "2.2",
                "--hem-bending-mult",
                "4.0",
                "--seam-width-mm",
                "12.0",
                "--seam-mass-mult",
                "2.0",
                "--seam-bending-mult",
                "3.5",
                "--start-self-repulsion",
                "--start-repulsion-mm",
                "3.0",
                "--start-repulsion-stiffness",
                "0.5",
                "--start-repulsion-period",
                "2",
                "--start-repulsion-max-checks",
                "500000",
            ],
        ] {
            let parsed = parse(&cmd_argv.iter().map(|s| (*s).into()).collect::<Vec<_>>()).unwrap();
            let err = if cmd_argv[0] == "drape" {
                cmd_drape(&parsed).unwrap_err()
            } else {
                round::cmd_round(&parsed).unwrap_err()
            };
            assert!(
                !err.message.contains("has no option") && !err.message.contains("unknown option"),
                "{} must recognize FTX-27d/e/f/g options: {}",
                cmd_argv[0],
                err.message
            );
        }

        for bad in [
            vec!["--bending-model", "bogus"],
            vec!["--compression-model", "bogus"],
            vec!["--air-cn", "1.2"],
            vec!["--air-velocity", "1,0,0"],
            vec!["--air-drag", "--air-velocity", "1,2"],
            vec!["--air-drag", "--air-velocity", "1,2,3,4"],
            vec!["--air-drag", "--air-velocity", "NaN,0,0"],
            vec!["--air-drag", "--air-velocity", "inf,0,0"],
            vec!["--air-drag", "--air-velocity", "1e120,0,0"],
            vec!["--air-drag", "--air-cn", "-0.1"],
            vec!["--air-drag", "--air-rho", "NaN"],
            vec!["--air-drag", "--air-velocity", ""],
            vec!["--air-drag", "--air-velocity", "1,,3"],
            vec!["--air-drag", "--air-velocity", "1,2,3,"],
            vec!["--air-drag", "--air-velocity", "1,2,1e400"],
            vec!["--rayleigh-membrane-s", "-0.001"],
            vec!["--rayleigh-bending-s", "NaN"],
            vec!["--stick-slip-mus", "0.5"],
            vec!["--stick-slip-v", "0.02"],
            vec!["--stick-slip-mus", "0.2", "--stick-slip-muk", "0.4"],
            vec![
                "--stick-slip-mus",
                "0.5",
                "--stick-slip-muk",
                "0.3",
                "--stick-slip-v",
                "0.0",
            ],
            vec!["--self-stick-slip-muk", "0.2"],
            vec![
                "--self-stick-slip-mus",
                "0.2",
                "--self-stick-slip-muk",
                "0.5",
            ],
            vec!["--min-spacing-mm", "0.0"],
            vec!["--max-turning-angle-rad", "0.2"],
            vec!["--gather-refinement", "2.5"],
            vec!["--cvt-iters", "99"],
            vec!["--hem-width-mm", "15.0"],
            vec!["--hem-mass-mult", "0.5"],
            vec!["--seam-width-mm", "10.0"],
            vec!["--start-repulsion-mm", "3.0"],
            vec!["--start-repulsion-max-checks", "1000"],
            vec!["--start-self-repulsion", "--start-repulsion-mm", "0.0"],
            vec!["--start-self-repulsion", "--start-repulsion-period", "0"],
            vec![
                "--start-self-repulsion",
                "--start-repulsion-max-checks",
                "0",
            ],
            vec![
                "--start-self-repulsion",
                "--start-repulsion-max-checks",
                "128000001",
            ],
            vec![
                "--start-self-repulsion",
                "--start-repulsion-max-checks",
                "many",
            ],
        ] {
            let mut argv = vec!["drape", "absent-pattern.json"];
            argv.extend(bad.iter().copied());
            let args = parse(&argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            let err = cmd_drape(&args).unwrap_err();
            assert_ne!(err.code, RefusalCode::Io, "{}", err.message);
            assert!(
                !err.message.contains("has no option") && !err.message.contains("unknown option"),
                "drape must recognize physics realism options: {}",
                err.message
            );

            let mut round_argv = vec!["round", "absent-ws", "--sim"];
            round_argv.extend(bad.iter().copied());
            let round_args =
                parse(&round_argv.into_iter().map(String::from).collect::<Vec<_>>()).unwrap();
            let round_err = round::cmd_round(&round_args).unwrap_err();
            assert_ne!(round_err.code, RefusalCode::Io, "{}", round_err.message);
            assert!(
                !round_err.message.contains("has no option")
                    && !round_err.message.contains("unknown option"),
                "round must recognize physics realism options: {}",
                round_err.message
            );
        }
    }

    #[test]
    fn ambient_wind_cli_is_preserved_for_drape_round_and_qualification() {
        for command in ["drape", "round", "qualify"] {
            let args = parse(
                &[command, "--air-drag", "--air-velocity", "-2.5, 0.25,1.5"].map(String::from),
            )
            .unwrap();
            let options = drape_options(&args).unwrap();
            let drag = options.damping.air_drag.unwrap();
            assert_eq!(drag.air_velocity_m_s, [-2.5, 0.25, 1.5]);
            let encoded = serde_json::to_value(options.damping).unwrap();
            assert_eq!(
                encoded["air_drag"]["air_velocity_m_s"],
                json!([-2.5, 0.25, 1.5])
            );
        }

        let still =
            drape_options(&parse(&["drape", "--air-drag"].map(String::from)).unwrap()).unwrap();
        let zero = drape_options(
            &parse(&["drape", "--air-drag", "--air-velocity", "0,0,0"].map(String::from)).unwrap(),
        )
        .unwrap();
        assert_eq!(still.damping, zero.damping);
        assert!(
            serde_json::to_value(zero.damping).unwrap()["air_drag"]
                .get("air_velocity_m_s")
                .is_none()
        );

        for argv in [
            vec![
                "drape",
                "absent-pattern.json",
                "--air-drag",
                "--air-velocity",
                "-2.5,0.25,1.5",
            ],
            vec![
                "round",
                "absent-ws",
                "--pattern",
                "absent-pattern.json",
                "--sim",
                "--air-drag",
                "--air-velocity",
                "-2.5,0.25,1.5",
            ],
        ] {
            let args = parse(&argv.iter().map(|s| (*s).into()).collect::<Vec<_>>()).unwrap();
            let error = if argv[0] == "drape" {
                cmd_drape(&args).unwrap_err()
            } else {
                round::cmd_round(&args).unwrap_err()
            };
            assert_eq!(error.code, RefusalCode::Io, "{error}");
        }
        let no_sim = parse(
            &[
                "round",
                "absent-ws",
                "--air-drag",
                "--air-velocity",
                "1,0,0",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_ne!(round::cmd_round(&no_sim).unwrap_err().code, RefusalCode::Io);
    }

    #[test]
    fn body_validate_and_drape_refuse_extra_positional_arguments() {
        let body_args = parse(
            &[
                "body",
                "stray.json",
                "--measurements",
                "absent.json",
                "--out",
                "absent-out",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_body(&body_args).unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        let validate_args = parse(
            &[
                "validate",
                "a.json",
                "b.json",
                "--measurements",
                "absent.json",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_validate(&validate_args).unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        let drape_args = parse(
            &[
                "drape",
                "a.json",
                "b.json",
                "--measurements",
                "absent.json",
                "--out",
                "absent-out",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_drape(&drape_args).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
    }

    #[test]
    fn core_cli_readers_refuse_directories_oversized_and_unix_fifos() {
        let dir = std::env::temp_dir().join(format!(
            "ftx-cli-core-readers-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let dir_str = dir.to_str().unwrap();

        let missing = read_regular_bounded(&dir.join("absent.json"), 16, "JSON").unwrap_err();
        assert_eq!(missing.code, RefusalCode::Io);

        let dir_err = read_regular_bounded(&dir, 16, "JSON").unwrap_err();
        assert_eq!(dir_err.code, RefusalCode::InvalidDocument);
        assert!(dir_err.message.contains("regular file"));

        let exact = dir.join("exact.json");
        std::fs::write(&exact, br#"{"ok":true}"#).unwrap();
        let (value, raw) = read_json(exact.to_str().unwrap()).unwrap();
        assert_eq!(value["ok"], true);
        assert_eq!(raw, br#"{"ok":true}"#);

        assert_eq!(
            fold::bytes(&dir, 64).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let fabric_dir =
            parse(&["drape", "p.json", "--fabric", dir_str].map(String::from)).unwrap();
        assert_eq!(
            fabric::load(&fabric_dir)
                .err()
                .expect("directory fabric input must refuse")
                .code,
            RefusalCode::InvalidDocument
        );
        let import_dir = parse(
            &[
                "import",
                "garmentcode",
                dir_str,
                "--out",
                dir.join("new-import").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            import::cmd_import(&import_dir).unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        let big = dir.join("big.json");
        std::fs::write(&big, vec![b'a'; 65]).unwrap();
        assert_eq!(
            read_regular_bounded(&big, 64, "JSON").unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        assert_eq!(
            fold::bytes(&big, 64).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );

        #[cfg(unix)]
        {
            let fifo = dir.join("input.fifo");
            let created = std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .is_ok_and(|s| s.success());
            assert!(created, "mkfifo should create test FIFO");
            let fifo_str = fifo.to_str().unwrap();
            assert_eq!(
                read_regular_bounded(&fifo, 64, "JSON").unwrap_err().code,
                RefusalCode::InvalidDocument
            );
            assert_eq!(
                fold::bytes(&fifo, 64).unwrap_err().code,
                RefusalCode::InvalidDocument
            );
            let fabric_fifo =
                parse(&["drape", "p.json", "--fabric", fifo_str].map(String::from)).unwrap();
            assert_eq!(
                fabric::load(&fabric_fifo)
                    .err()
                    .expect("FIFO fabric input must refuse")
                    .code,
                RefusalCode::InvalidDocument
            );
            let import_fifo = parse(
                &[
                    "import",
                    "garmentcode",
                    fifo_str,
                    "--out",
                    dir.join("new-import-fifo").to_str().unwrap(),
                ]
                .map(String::from),
            )
            .unwrap();
            assert_eq!(
                import::cmd_import(&import_fifo).unwrap_err().code,
                RefusalCode::InvalidDocument
            );
        }
    }
}
