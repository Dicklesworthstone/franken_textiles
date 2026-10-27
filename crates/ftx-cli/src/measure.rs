//! Measurement evidence workflow. Library computations are clock-free; the CLI defaults to UTC today.

mod body_fit;
mod bundle;
mod calibration;
mod intrinsics;
mod photo;
mod section;
mod still;
mod triangulate;
mod video;
mod video_decode;
mod video_masks;

use super::{Args, bad_input, known_options, read_json, required};
use ftx_core::{Refusal, RefusalCode};
use ftx_measure::{
    NAMES, ReadinessOptions, canonical_name, catalogue, date_day, definition, fuse,
    garment_catalogue, make_record, parse_value, priors_for, readiness, reference_wearer, unit,
    validate,
};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::atomic::{AtomicU64, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

pub(super) const BOOLEAN_OPTIONS: &[&str] = &[
    "no-priors",
    "passes-hip",
    "closed-cuff",
    "nappies",
    "no-nappies",
    "elastic-waist",
    "cuff-opening",
];

fn option<'a>(args: &'a Args, key: &str) -> Option<&'a str> {
    args.options.get(key).map(String::as_str)
}
fn flag(args: &Args, key: &str) -> bool {
    args.options.contains_key(key)
}
fn numeric(args: &Args, key: &str) -> Result<Option<f64>, Refusal> {
    option(args, key)
        .map(|s| {
            let v = s
                .parse::<f64>()
                .map_err(|_| bad_input(format!("--{key} must be a finite number")))?;
            Refusal::finite(key, v)
        })
        .transpose()
}
fn utc_date() -> Result<String, Refusal> {
    let days = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| bad_input("clock precedes 1970; give an explicit date"))?
        .as_secs()
        / 86400;
    let ordinal = i64::try_from(days).map_err(|_| bad_input("clock out of range"))? + 719163;
    // Bounded search over the ISO calendar, reusing the core's tested Gregorian conversion.
    let (mut lo, mut hi) = (1970, 10000);
    while lo + 1 < hi {
        let mid = (lo + hi) / 2;
        if date_day(&format!("{mid:04}-01-01")).unwrap() <= ordinal {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    for month in (1..=12).rev() {
        let first = date_day(&format!("{lo:04}-{month:02}-01")).unwrap();
        if first <= ordinal {
            let date = format!("{lo:04}-{month:02}-{:02}", ordinal - first + 1);
            return date_day(&date)
                .map(|_| date)
                .ok_or_else(|| bad_input("clock out of ISO date range"));
        }
    }
    Err(bad_input("clock out of range"))
}
pub(super) fn date(args: &Args, key: &str) -> Result<String, Refusal> {
    let date = option(args, key)
        .map(str::to_string)
        .map_or_else(utc_date, Ok)?;
    if date_day(&date).is_none() {
        return Err(bad_input(format!("--{key} must be YYYY-MM-DD")));
    }
    Ok(date)
}
fn io_error(path: &Path, e: std::io::Error) -> Refusal {
    Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display()))
}
fn write_json(path: &str, value: &Value) -> Result<(), Refusal> {
    // Serialize and compute completely before touching the destination. Rename a private same-dir
    // file so an in-place `add` cannot truncate the original evidence on validation/write failure.
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = Path::new(path);
    let parent = path
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|e| bad_input(e.to_string()))?;
    bytes.push(b'\n');
    std::fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
    let name = path
        .file_name()
        .ok_or_else(|| bad_input("output must name a file"))?
        .to_string_lossy();
    let temp = parent.join(format!(
        ".{name}.ftx-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temp).map_err(|e| io_error(&temp, e))?;
    file.write_all(&bytes)
        .and_then(|()| file.sync_all())
        .map_err(|e| io_error(&temp, e))?;
    drop(file);
    std::fs::rename(&temp, path).map_err(|e| io_error(path, e))?;
    Ok(())
}
fn checked(value: &Value) -> Result<(), Refusal> {
    let errors = validate(value);
    if errors.is_empty() {
        Ok(())
    } else {
        Err(bad_input(errors.join("; ")))
    }
}
fn merged(paths: &[String]) -> Result<Value, Refusal> {
    let first = paths
        .first()
        .ok_or_else(|| bad_input("give at least one measurement FILE"))?;
    let mut base = read_json(first)?.0;
    checked(&base)?;
    for path in &paths[1..] {
        let next = read_json(path)?.0;
        checked(&next)?;
        if base["wearer"]
            .as_object()
            .unwrap()
            .values()
            .all(Value::is_null)
        {
            base["wearer"] = next["wearer"].clone();
        }
        if base["taken_on"].is_null() {
            base["taken_on"] = next["taken_on"].clone();
        }
        base["records"]
            .as_array_mut()
            .unwrap()
            .extend(next["records"].as_array().unwrap().iter().cloned());
    }
    base.as_object_mut().unwrap().remove("photo");
    Ok(base)
}
fn single<'a>(args: &'a Args, command: &str) -> Result<&'a str, Refusal> {
    if args.positional.len() != 3 {
        return Err(bad_input(format!(
            "ftx measure {command} takes exactly one FILE"
        )));
    }
    Ok(&args.positional[2])
}
fn no_files(args: &Args, command: &str) -> Result<(), Refusal> {
    if args.positional.len() != 2 {
        Err(bad_input(format!(
            "ftx measure {command} takes no positional FILE"
        )))
    } else {
        Ok(())
    }
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let command = args
        .positional
        .get(1)
        .map(String::as_str)
        .ok_or_else(|| bad_input("give a measure subcommand"))?;
    if command == "video-select" {
        return video::run(args);
    }
    if command == "video-decode" {
        return video_decode::run(args);
    }
    if command == "video-mask-review" {
        return video_masks::review(args);
    }
    if command == "video-masks" {
        return video_masks::apply(args);
    }
    if command == "capture-review" {
        return body_fit::capture_review(args);
    }
    if command == "fit-start-review" {
        return body_fit::fit_start_review(args);
    }
    if command == "still-window" {
        return still::run(args);
    }
    let mut output = option(args, "out");
    let mut code = 0;
    let value = match command {
        "calibrate-camera" => return intrinsics::run(args),
        "fit-body" => return body_fit::run(args),
        "bundle-adjust" => return bundle::run(args),
        "calib-sheet" => return calibration::run(args),
        "triangulate" => return triangulate::run(args),
        "girth-from-section" => section::run(args)?,
        "photo-scale" | "measure-photo" | "sheet-pose" => {
            let (value, status) = photo::run(args, command)?;
            code = status;
            value
        }
        "catalogue" => {
            known_options(args, "measure catalogue", &["garment", "out"])?;
            no_files(args, command)?;
            match option(args, "garment") {
                Some(g) => garment_catalogue(g)?,
                None => catalogue().clone(),
            }
        }
        "prior" | "reference" => {
            known_options(
                args,
                "measure prior/reference",
                &["stature-mm", "age-months", "sex", "label", "out"],
            )?;
            no_files(args, command)?;
            let stature = numeric(args, "stature-mm")?
                .ok_or_else(|| bad_input("--stature-mm is required"))?;
            let age = numeric(args, "age-months")?;
            if command == "prior" {
                priors_for(stature, age, option(args, "sex"))?
            } else {
                reference_wearer(stature, age, option(args, "sex"), option(args, "label"))?
            }
        }
        "init" => {
            known_options(
                args,
                "measure init",
                &[
                    "out",
                    "label",
                    "age-months",
                    "sex",
                    "stature",
                    "taken-on",
                    "nappies",
                ],
            )?;
            no_files(args, command)?;
            required(args, "out")?;
            let label = required(args, "label")?;
            let h = option(args, "stature")
                .map(|s| parse_value(s, "mm"))
                .transpose()?;
            let mut d = json!({"schema":"fabric-measurements/1","wearer":{"label":label,
                "age_months":numeric(args,"age-months")?,"stature_mm":h},"taken_on":date(args,"taken-on")?,"records":[],"fused":{}});
            if let Some(sex) = option(args, "sex") {
                d["wearer"]["sex"] = json!(sex);
            }
            if flag(args, "nappies") {
                d["wearer"]["nappies"] = json!(true);
            }
            if let Some(h) = h {
                d["records"] = json!([make_record(
                    "stature",
                    h,
                    10.0,
                    "MEASURED_TAPE",
                    "wall + book (init)",
                    None,
                    ""
                )?]);
            }
            checked(&d)?;
            d
        }
        "add" => {
            known_options(
                args,
                "measure add",
                &[
                    "name",
                    "value",
                    "sigma",
                    "provenance",
                    "method",
                    "definition-id",
                    "notes",
                    "out",
                ],
            )?;
            let path = single(args, command)?;
            let mut d = read_json(path)?.0;
            checked(&d)?;
            let name = canonical_name(required(args, "name")?);
            let u = unit(&name);
            let provenance = option(args, "provenance").unwrap_or("MEASURED_TAPE");
            let sigma = if let Some(s) = option(args, "sigma") {
                parse_value(s, u)?
            } else {
                let default = if provenance == "MEASURED_TAPE" {
                    definition(&name).and_then(|d| d["tape_sigma"].as_f64())
                } else if u == "mm" {
                    match provenance {
                        "GARMENT_DERIVED" => Some(18.0),
                        "MODEL_FIT" => Some(12.0),
                        "SCAN_MEASURED" => Some(15.0),
                        _ => None,
                    }
                } else {
                    None
                };
                default.ok_or_else(|| {
                    bad_input(format!(
                        "give --sigma for {name} ({provenance} has no default)"
                    ))
                })?
            };
            if args.values.is_empty() {
                return Err(bad_input("give at least one --value with a unit"));
            }
            for v in &args.values {
                let r = make_record(
                    &name,
                    parse_value(v, u)?,
                    sigma,
                    provenance,
                    option(args, "method").unwrap_or(""),
                    option(args, "definition-id"),
                    option(args, "notes").unwrap_or(""),
                )?;
                d["records"].as_array_mut().unwrap().push(r);
            }
            d["fused"] = json!({});
            d.as_object_mut().unwrap().remove("fusion");
            output = Some(output.unwrap_or(path));
            d
        }
        "validate" => {
            known_options(args, "measure validate", &[])?;
            let path = single(args, command)?;
            let d = read_json(path)?.0;
            let errors = validate(&d);
            if !errors.is_empty() {
                code = 3;
            }
            json!({"file":path,"valid":errors.is_empty(),"errors":errors})
        }
        "fuse" => {
            known_options(args, "measure fuse", &["no-priors", "out"])?;
            fuse(&merged(&args.positional[2..])?, !flag(args, "no-priors"))?
        }
        "ready" => {
            known_options(
                args,
                "measure ready",
                &[
                    "garment",
                    "passes-hip",
                    "closed-cuff",
                    "nappies",
                    "no-nappies",
                    "elastic-waist",
                    "cuff-level",
                    "cuff-opening",
                    "as-of",
                    "no-priors",
                    "out",
                ],
            )?;
            if flag(args, "nappies") && flag(args, "no-nappies") {
                return Err(bad_input("choose --nappies or --no-nappies, not both"));
            }
            let options = ReadinessOptions {
                passes_hip: flag(args, "passes-hip"),
                closed_cuff: flag(args, "closed-cuff"),
                nappies: if flag(args, "nappies") {
                    Some(true)
                } else if flag(args, "no-nappies") {
                    Some(false)
                } else {
                    None
                },
                elastic_waist: flag(args, "elastic-waist"),
                cuff_level: option(args, "cuff-level").unwrap_or("calf"),
                cuff_opening: flag(args, "cuff-opening"),
                use_priors: !flag(args, "no-priors"),
            };
            let d = readiness(
                &merged(&args.positional[2..])?,
                required(args, "garment")?,
                &options,
                &date(args, "as-of")?,
            )?;
            if d["status"] == "NOT_READY" {
                code = 3;
            }
            d
        }
        _ => {
            return Err(bad_input(format!(
                "unknown measure command '{command}'; use catalogue, prior, init, add, validate, fuse, ready, reference, calib-sheet, calibrate-camera, photo-scale, measure-photo, sheet-pose, girth-from-section, triangulate, bundle-adjust, fit-body, capture-review, fit-start-review, still-window, video-decode, video-mask-review, video-masks or video-select"
            )));
        }
    };
    let text = if command == "ready" {
        let mut text = format!(
            "MEASUREMENTS: {} for {}",
            value["status"].as_str().unwrap_or(""),
            value["garment"].as_str().unwrap_or("")
        );
        if let Some(note) = value["note"].as_str() {
            text.push_str(&format!("\n{note}"));
        }
        for key in ["remeasure_request", "for_real_child", "advisories"] {
            if let Some(lines) = value[key].as_array() {
                for line in lines {
                    text.push_str(&format!("\n  {}", line.as_str().unwrap_or("")));
                }
            }
        }
        text
    } else if output.is_some() {
        // the document goes to the file, as measurements.py's does ("wrote FILE" on stderr)
        String::new()
    } else if command == "catalogue" && option(args, "garment").is_none() {
        NAMES
            .iter()
            .map(|n| {
                format!(
                    "{n} ({}): {}",
                    unit(n),
                    catalogue()[n]["how"].as_str().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        serde_json::to_string_pretty(&value).map_err(|e| bad_input(e.to_string()))?
    };
    if let Some(path) = output {
        if matches!(
            command,
            "photo-scale" | "measure-photo" | "sheet-pose" | "girth-from-section"
        ) {
            photo::save(path, &value)?;
        } else {
            write_json(path, &value)?;
        }
        eprintln!("wrote {path}");
    }
    Ok((value, code, text))
}
