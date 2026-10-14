//! `ftx annotate`, `ftx scale-fit` and `ftx audit`: the reference's `vectorize.py` commands on a traced draft
//! (spec 04 §10.1), on `ftx-trace`. Documents are read as text so vertex and `names.json` order survive.

use std::path::Path;

use ftx_core::{Refusal, RefusalCode, py};
use ftx_pattern::doc::PatternDoc;
use ftx_trace::{AuditOptions, OValue, annotate, audit, fit_scale, parse_length};
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir, required, workspace};

fn unused(path: &Path) -> Result<(), Refusal> {
    match std::fs::symlink_metadata(path) {
        Ok(_) => Err(bad_input(format!(
            "{} already exists; choose a new review output",
            path.display()
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(workspace::io_error(path, e)),
    }
}

fn save_new(path: &Path, bytes: &[u8]) -> Result<(), Refusal> {
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        make_dir(parent)?;
    }
    workspace::write_new(path, bytes)
}

fn publish(out: &Path, files: &[(String, Vec<u8>)]) -> Result<(), Refusal> {
    // Compute every artifact before publication. Existing unrelated files in the destination
    // are allowed, but no artifact or dangling link may be replaced. JSON is written last.
    if std::fs::symlink_metadata(out).is_ok_and(|m| m.file_type().is_symlink()) {
        return Err(bad_input("review output directory must not be a symlink"));
    }
    for (name, _) in files {
        unused(&out.join(name))?;
    }
    for (name, bytes) in files {
        save_new(&out.join(name), bytes)?;
    }
    Ok(())
}

fn source_image(args: &Args) -> Result<Option<ftx_trace::raster::Bgr>, Refusal> {
    args.options
        .get("image")
        .map(|p| {
            // Never follow a filename embedded in pattern metadata. Only --image authorizes I/O.
            let bytes = std::fs::read(p).map_err(|e| workspace::io_error(Path::new(p), e))?;
            ftx_trace::raster::image::load_image_with_limit(&bytes, 16_000_000).map_err(bad_input)
        })
        .transpose()
}

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
    known_options(args, "annotate", &["names", "out", "overlay", "image"])?;
    if args.options.contains_key("image") && !args.options.contains_key("overlay") {
        return Err(bad_input("annotate --image requires --overlay PNG"));
    }
    let draft = read_doc(input(args, "annotate", "a traced draft")?)?;
    let names_path = required(args, "names")?;
    let out = required(args, "out")?;
    let names: OValue = serde_json::from_str(&read_text(names_path)?)
        .map_err(|e| bad_input(format!("{names_path}: {e}")))?;
    let data = annotate(&draft, &names).map_err(bad_input)?;
    if let Some(path) = args.options.get("overlay") {
        unused(Path::new(out))?;
        unused(Path::new(path))?;
        if Path::new(out) == Path::new(path) {
            return Err(bad_input("pattern and overlay must be different files"));
        }
        let image = source_image(args)?;
        let png = ftx_trace::review::overlay(&data, image.as_ref())?;
        // Serialize the typed document directly: to_value would sort the ordered vertex map.
        let mut bytes = serde_json::to_vec_pretty(&data).map_err(|e| bad_input(e.to_string()))?;
        bytes.push(b'\n');
        save_new(Path::new(path), &png)?;
        save_new(Path::new(out), &bytes)?;
    } else {
        write_json(Path::new(out), &data)?;
    }
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
        &[
            "out",
            "labels",
            "tol-mm",
            "median-seam-mm",
            "label-tol",
            "image",
        ],
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
    let source = source_image(args)?;
    let png = ftx_trace::review::audit_image(&doc, &report, source.as_ref())?;
    let file = out.join("round0_audit.json");
    let picture = out.join("round0_audit.png");
    publish(
        out,
        &[
            ("round0_audit.png".into(), png),
            ("round0_audit.json".into(), workspace::json_bytes(&report)?),
        ],
    )?;
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
                        "findings": findings, "files": [file.display().to_string(),picture.display().to_string()]});
    Ok((result, if ok { 0 } else { 2 }, text))
}

/// `ftx trace`'s flags.
pub(super) const BOOLEAN_OPTIONS: &[&str] = &["invert", "no-invert", "no-dashes"];

fn numbers<const N: usize>(key: &str, s: &str) -> Result<[f64; N], Refusal> {
    let v: Vec<f64> = s
        .split(',')
        .map(|p| p.trim().parse::<f64>())
        .collect::<Result<_, _>>()
        .map_err(|_| {
            bad_input(format!(
                "--{key} takes {N} comma-separated numbers, got {}",
                py::repr_str(s)
            ))
        })?;
    v.try_into().map_err(|_| {
        bad_input(format!(
            "--{key} takes {N} comma-separated numbers, got {}",
            py::repr_str(s)
        ))
    })
}

fn number<T: std::str::FromStr>(args: &Args, key: &str) -> Result<Option<T>, Refusal> {
    args.options
        .get(key)
        .map(|v| {
            v.parse::<T>()
                .map_err(|_| bad_input(format!("--{key}: invalid value {}", py::repr_str(v))))
        })
        .transpose()
}

/// `ftx trace IMAGE --out DIR [--crop X,Y,W,H] [--channel C] [--threshold otsu] [--bg R,G,B] [--stroke PX]
/// [--close PX] [--open PX] [--corner-deg D] [--soft-deg D] [--simplify-frac F] [--smooth SIGMA]
/// [--min-area-frac F] [--erase X,Y,W,H ...] [--invert | --no-invert] [--no-dashes]`: a pattern sheet (PNG or JPEG)
/// traced into an UNSCALED draft, the reference's `vectorize.py trace`. Writes `DIR/draft.pattern.json`
/// (the reference's text) and `DIR/mask.png`, and reports each piece as the reference does; exits 3 when no
/// piece was found. Review images use the native stroke renderer, not OpenCV's text pixels.
pub(super) fn cmd_trace(args: &Args) -> Result<(Value, i32, String), Refusal> {
    use ftx_trace::raster::image::{encode_png_gray, load_image};
    use ftx_trace::{TraceOptions, trace_image};
    known_options(
        args,
        "trace",
        &[
            "out",
            "crop",
            "min-area-frac",
            "channel",
            "invert",
            "no-invert",
            "threshold",
            "bg",
            "stroke",
            "close",
            "open",
            "corner-deg",
            "soft-deg",
            "simplify-frac",
            "smooth",
            "erase",
            "no-dashes",
            "px-out",
        ],
    )?;
    let image = input(args, "trace", "an image")?;
    let out = Path::new(required(args, "out")?);
    // A previous labels directory could contain extra pieces from another run.
    unused(&out.join("labels"))?;
    if args.options.contains_key("invert") && args.options.contains_key("no-invert") {
        return Err(bad_input(
            "argument --no-invert: not allowed with argument --invert",
        ));
    }
    let d = TraceOptions::default();
    let opts = TraceOptions {
        crop: args
            .options
            .get("crop")
            .map(|s| numbers::<4>("crop", s))
            .transpose()?,
        min_area_frac: number(args, "min-area-frac")?.unwrap_or(d.min_area_frac),
        channel: args.options.get("channel").cloned().unwrap_or(d.channel),
        invert: if args.options.contains_key("invert") {
            Some(true)
        } else if args.options.contains_key("no-invert") {
            Some(false)
        } else {
            None
        },
        threshold: args
            .options
            .get("threshold")
            .cloned()
            .unwrap_or(d.threshold),
        bg_rgb: args
            .options
            .get("bg")
            .map(|s| numbers::<3>("bg", s))
            .transpose()?,
        stroke: number(args, "stroke")?,
        close_px: number(args, "close")?.unwrap_or(d.close_px),
        open_px: number(args, "open")?,
        corner_deg: number(args, "corner-deg")?.unwrap_or(d.corner_deg),
        soft_deg: number(args, "soft-deg")?.unwrap_or(d.soft_deg),
        simplify_frac: number(args, "simplify-frac")?.unwrap_or(d.simplify_frac),
        smooth_px: number(args, "smooth")?.unwrap_or(d.smooth_px),
        erase: args
            .options
            .get("erase")
            .map(|s| {
                s.split('\n')
                    .map(|r| numbers::<4>("erase", r))
                    .collect::<Result<Vec<_>, _>>()
            })
            .transpose()?
            .unwrap_or_default(),
        marks: true,
        dashes: !args.options.contains_key("no-dashes"),
    };
    let bytes =
        std::fs::read(image).map_err(|e| Refusal::new(RefusalCode::Io, format!("{image}: {e}")))?;
    let img = load_image(&bytes).map_err(|e| bad_input(format!("{image}: {e}")))?;
    let source = std::fs::canonicalize(image)
        .map_or_else(|_| image.to_string(), |p| p.display().to_string());
    let traced = trace_image(&img, Some(&source), &opts).map_err(bad_input)?;
    let draft = out.join("draft.pattern.json");
    let mask = out.join("mask.png");
    let data = traced.pattern.to_value();
    // each piece's stroke-centre polygon (crop px, y down, closed): what the label crops are cut along
    let px = args
        .options
        .get("px-out")
        .map_or_else(|| out.join("trace_px.json"), std::path::PathBuf::from);
    let rings: serde_json::Map<String, Value> = data["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(&traced.polys)
        .map(|(p, ring)| {
            let pts: Vec<Value> = ring.iter().map(|q| json!([q.x, q.y])).collect();
            (
                p["id"].as_str().unwrap_or("").to_string(),
                Value::Array(pts),
            )
        })
        .collect();
    let px_doc = json!({"schema": "ftx-trace-px/1", "units": "crop px, y down", "pieces": rings});
    let doc: PatternDoc =
        serde_json::from_value(data.clone()).map_err(|e| bad_input(e.to_string()))?;
    let overlay = out.join("overlay.png");
    let label_dir = out.join("labels");
    let mut artifacts = vec![
        ("mask.png".into(), encode_png_gray(&traced.mask)),
        (
            "overlay.png".into(),
            ftx_trace::review::overlay(&doc, Some(&img))?,
        ),
    ];
    let px_bytes = (px_doc.to_string() + "\n").into_bytes();
    // `--px-out FILE` keeps the polygons out of DIR (the skill's wrapper, whose DIR matches vectorize.py's)
    match args.options.get("px-out") {
        Some(file) => std::fs::write(file, &px_bytes)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{file}: {e}")))?,
        None => artifacts.push(("trace_px.json".into(), px_bytes)),
    }
    for (i, label) in ftx_trace::review::label_crops(&traced, &img)?
        .into_iter()
        .enumerate()
    {
        if label.piece_id != format!("p{}", i + 1) {
            return Err(bad_input("trace label ids must follow pN order"));
        }
        artifacts.push((format!("labels/{}.png", label.piece_id), label.png));
    }
    let mut pieces = Vec::new();
    let mut lines = Vec::new();
    for (p, piece) in data["pieces"]
        .as_array()
        .into_iter()
        .flatten()
        .zip(&doc.pieces)
    {
        let area = ftx_trace::piece_instance(piece)
            .area()
            .map_err(|e| bad_input(e.message().to_string()))?;
        let count = |k: &str| p[k].as_array().map_or(0, Vec::len);
        let curved = p["edges"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|e| e.get("curve").is_some())
            .count();
        let row = json!({
            "id": p["id"], "mode": p["trace"]["mode"], "stroke_px": p["trace"]["stroke_px"],
            "vertices": p["vertices"].as_object().map_or(0, serde_json::Map::len), "edges": count("edges"),
            "curved": curved, "notches": count("notches"), "grain": p.get("grain").is_some(),
            "touches_border": p["trace"]["touches_border"], "area_px": py::round(area, 1),
        });
        lines.push(format!(
            "  {} {} stroke {} px: {} vertices, {} edges ({curved} curved), {} notches{}{}",
            p["id"].as_str().unwrap_or("?"),
            p["trace"]["mode"].as_str().unwrap_or("?"),
            p["trace"]["stroke_px"],
            row["vertices"],
            row["edges"],
            row["notches"],
            if row["grain"] == true { ", grain" } else { "" },
            if p["trace"]["touches_border"] == true {
                ", touches the border"
            } else {
                ""
            },
        ));
        pieces.push(row);
    }
    let diag = traced.diagnostics.to_value();
    artifacts.push((
        "draft.pattern.json".into(),
        (traced.pattern.dumps(Some(1)) + "\n").into_bytes(),
    ));
    publish(out, &artifacts)?;
    let files = json!({"draft": draft.display().to_string(), "mask": mask.display().to_string(),
                       "polygons": px.display().to_string(),"overlay":overlay.display().to_string(),"labels":label_dir.display().to_string()});
    let text = format!(
        "traced {} pieces -> {}{}{}",
        pieces.len(),
        draft.display(),
        if lines.is_empty() {
            String::new()
        } else {
            format!("\n{}", lines.join("\n"))
        },
        diag["retries"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(|r| format!("\n  retry: {r}"))
            .collect::<String>()
    );
    let code = if pieces.is_empty() { 3 } else { 0 };
    Ok((
        json!({"pieces": pieces, "files": files, "bg_rgb": diag["bg_rgb"], "retries": diag["retries"]}),
        code,
        text,
    ))
}
