//! `ftx export TARGET PATTERN --out DIR` and `ftx check DIR`: the reference's `export.py` (spec 03 §1.3) on
//! `ftx-export`.

use std::path::{Component, Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ftx_core::{Refusal, RefusalCode};
use ftx_export::{
    CheckItem, Ctx, ExportOptions, PyJson, Target, check_files, export_targets, read_dir_files,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir};

/// The export flags that take no value.
pub(super) const BOOLEAN_OPTIONS: &[&str] =
    &["unfold", "keep-blank-pages", "hpgl-rotate90", "hpgl-mirror"];

const TARGET_NAMES: &str = "svg, pdf, letter, a4, copyshop, projector, dxf, astm, aama, flat-dxf, hpgl, cutter, garmentcode, embroidery, all, check";

/// Now in UTC as `YYYY-MM-DDTHH:MM` (the reference uses local time; `--created` pins it).
pub(super) fn utc_minute() -> Result<String, Refusal> {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| bad_input("clock precedes 1970; pass --created"))?
        .as_secs();
    let secs = i64::try_from(secs).map_err(|_| bad_input("clock out of range; pass --created"))?;
    // days since 1970-01-01 to the proleptic Gregorian date (H. Hinnant's civil_from_days)
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    let minute = secs.rem_euclid(86_400) / 60;
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}",
        minute / 60,
        minute % 60
    ))
}

fn number(args: &Args, key: &str) -> Result<Option<f64>, Refusal> {
    args.options
        .get(key)
        .map(|v| {
            let x: f64 = v
                .parse()
                .map_err(|_| bad_input(format!("--{key} must be a number")))?;
            if x.is_finite() && x > 0.0 {
                Ok(x)
            } else {
                Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("--{key} must be a positive number, got {v}"),
                ))
            }
        })
        .transpose()
}

fn nonnegative_number(args: &Args, key: &str) -> Result<Option<f64>, Refusal> {
    args.options
        .get(key)
        .map(|v| {
            let x: f64 = v
                .parse()
                .map_err(|_| bad_input(format!("--{key} must be a number")))?;
            if x.is_finite() && x >= 0.0 {
                Ok(x)
            } else {
                Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    format!("--{key} must be a finite non-negative number, got {v}"),
                ))
            }
        })
        .transpose()
}

fn load_pattern(path: &str) -> Result<Pattern, Refusal> {
    Ok(Pattern::new(
        crate::fold::pattern_document(Path::new(path))?.0,
    ))
}

fn check_export_destination(out: &Path, rel: &str) -> Result<PathBuf, Refusal> {
    let rel_path = Path::new(rel);
    let mut cur = out.to_path_buf();
    let mut parts = Vec::new();
    for component in rel_path.components() {
        match component {
            Component::Normal(part) => parts.push(part),
            _ => {
                return Err(bad_input(format!(
                    "export output path {rel:?} must stay inside --out"
                )));
            }
        }
    }
    let Some((file, dirs)) = parts.split_last() else {
        return Err(bad_input("export output path must name a file"));
    };
    for dir in dirs {
        cur.push(dir);
        crate::round::regular(&cur, true)?;
    }
    cur.push(file);
    crate::round::regular(&cur, false)?;
    Ok(cur)
}

/// The checks' summary line and failure lines, as the reference prints them.
fn summary(items: &[CheckItem]) -> (bool, usize, String) {
    let passed = items.iter().filter(|i| i.ok).count();
    let ok = passed == items.len();
    let mut text: Vec<String> = items
        .iter()
        .filter(|i| !i.ok)
        .map(|i| format!("FAIL {} [{}] {}", i.check, i.file, i.detail))
        .collect();
    text.push(format!(
        "{}: {passed}/{} checks",
        if ok { "PASS" } else { "FAIL" },
        items.len()
    ));
    (ok, passed, text.join("\n"))
}

fn checks_json(items: &[CheckItem]) -> Value {
    let (ok, passed, _) = summary(items);
    json!({
        "status": if ok { "PASS" } else { "FAIL" },
        "passed": passed,
        "total": items.len(),
        "failures": items.iter().filter(|i| !i.ok).map(CheckItem::to_json).collect::<Vec<_>>(),
    })
}

/// `ftx check DIR [--pattern-ref P]` (also `ftx export check DIR`): every acceptance check over an export
/// directory; exit 0 when all pass, 2 otherwise (the reference exits 1).
pub(super) fn cmd_check(args: &Args, dirs: &[String]) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "check", &["pattern-ref"])?;
    if dirs.len() != 1 {
        return Err(bad_input("ftx check needs exactly one export directory"));
    }
    let dir = &dirs[0];
    let pattern = args
        .options
        .get("pattern-ref")
        .map(|p| load_pattern(p))
        .transpose()?;
    let files = read_dir_files(Path::new(dir))?;
    let items = check_files(&files, dir, pattern.as_ref());
    let (ok, _, text) = summary(&items);
    let mut result = checks_json(&items);
    result["dir"] = json!(dir);
    result["items"] = items.iter().map(CheckItem::to_json).collect();
    Ok((result, if ok { 0 } else { 2 }, text))
}

/// `ftx export TARGET PATTERN --out DIR [options]`: the target's files written into DIR and the manifest there
/// updated (an existing one keeps its other entries). `all` runs every target and then the checks with the
/// pattern (exit 2 when one fails). Nothing is written when a target refuses.
pub(super) fn cmd_export(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let target = args.positional.get(1).map_or("", String::as_str);
    if target == "check" {
        return cmd_check(args, args.positional.get(2..).unwrap_or(&[]));
    }
    known_options(
        args,
        "export",
        &[
            "out",
            "unfold",
            "sa",
            "mitre-limit",
            "projector-width",
            "layout-width",
            "keep-blank-pages",
            "hpgl-rotate90",
            "hpgl-mirror",
            "created",
        ],
    )?;
    let targets: Vec<Target> = if target == "all" {
        Target::ALL.to_vec()
    } else {
        vec![Target::parse(target).ok_or_else(|| {
            bad_input(format!("ftx export: no target {target:?}"))
                .repair(format!("targets: {TARGET_NAMES}"))
        })?]
    };
    if args.positional.len() != 3 {
        return Err(bad_input(format!(
            "ftx export {target} needs exactly one pattern file"
        )));
    }
    let pattern_path = &args.positional[2];
    let out = PathBuf::from(super::required(args, "out")?);
    let flag = |k: &str| args.options.contains_key(k);
    let opts = ExportOptions {
        unfold: flag("unfold"),
        mitre_limit: number(args, "mitre-limit")?.unwrap_or(4.0),
        projector_width: number(args, "projector-width")?.unwrap_or(1500.0),
        layout_width: number(args, "layout-width")?,
        keep_blank: flag("keep-blank-pages"),
        sa: nonnegative_number(args, "sa")?,
        hpgl_rotate90: flag("hpgl-rotate90"),
        hpgl_mirror: flag("hpgl-mirror"),
        created: args.options.get("created").cloned(),
    };
    let pattern = load_pattern(pattern_path)?;
    for e in pattern.structure_errors() {
        eprintln!("warning: {e}");
    }
    let mut opts = opts;
    if opts.created.is_none() && pattern.raw()["meta"]["created"].as_str().is_none() {
        opts.created = Some(utc_minute()?);
    }
    let ctx = Ctx::new(&pattern, opts)?;
    crate::round::regular(&out, true)?;
    let manifest_path = out.join(format!("{}_export_manifest.json", ctx.style));
    let existing = if crate::round::regular(&manifest_path, false)? {
        let text = std::fs::read_to_string(&manifest_path).map_err(|e| {
            Refusal::new(RefusalCode::Io, format!("{}: {e}", manifest_path.display()))
        })?;
        Some(
            PyJson::parse(&text)
                .map_err(|e| bad_input(format!("{}: {e}", manifest_path.display())))?,
        )
    } else {
        None
    };
    let set = export_targets(&ctx, &targets, existing)?;
    // Verify every output directory and file is non-symlink before creating or writing anything.
    let mut destinations = Vec::with_capacity(set.files.len());
    for (rel, bytes) in &set.files {
        destinations.push((check_export_destination(&out, rel)?, bytes.as_slice()));
    }
    make_dir(&out)?;
    let mut written = Vec::new();
    for (path, bytes) in destinations {
        if let Some(parent) = path.parent() {
            make_dir(parent)?;
        }
        std::fs::write(&path, bytes)
            .map_err(|e| Refusal::new(RefusalCode::Io, format!("{}: {e}", path.display())))?;
        if path != manifest_path {
            written.push(path.display().to_string());
        }
    }
    let mut text = written.join("\n");
    let mut result = json!({
        "out": out.display().to_string(),
        "targets": targets.iter().map(|t| format!("{t:?}").to_lowercase()).collect::<Vec<_>>(),
        "files": written,
        "manifest": manifest_path.display().to_string(),
    });
    let mut code = 0;
    if target == "all" {
        let files = read_dir_files(&out)?;
        let items = check_files(&files, &out.display().to_string(), Some(&pattern));
        let (ok, _, lines) = summary(&items);
        text = format!("{text}\n{lines}");
        result["checks"] = checks_json(&items);
        code = if ok { 0 } else { 2 };
    }
    Ok((result, code, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_minute_is_a_minute() {
        let s = super::utc_minute().unwrap();
        assert_eq!(s.len(), 16, "{s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
        assert!(s.as_str() >= "2026-01-01T00:00");
    }

    #[test]
    fn sa_option_accepts_zero_and_refuses_negative_or_non_finite() {
        for (raw, expected) in [("0", Some(0.0)), ("12.5", Some(12.5))] {
            let args =
                crate::parse(&["export", "svg", "p.json", "--sa", raw].map(String::from)).unwrap();
            assert_eq!(nonnegative_number(&args, "sa").unwrap(), expected);
        }
        for raw in ["-1", "-0.01", "NaN", "inf", "-inf", "abc"] {
            let args =
                crate::parse(&["export", "svg", "p.json", "--sa", raw].map(String::from)).unwrap();
            assert!(nonnegative_number(&args, "sa").is_err(), "--sa {raw}");
        }
    }

    #[cfg(unix)]
    #[test]
    fn export_refuses_symlinked_output_dir_and_files_before_writing() {
        use std::os::unix::fs::symlink;

        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root =
            std::env::temp_dir().join(format!("ftx-export-symlink-{}-{stamp}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../tests/fixtures/oracle/export/all.json"
        ))
        .unwrap();
        let pattern = &fixture["cases"][0]["input"]["pattern"];
        let pat_path = root.join("pattern.json");
        std::fs::write(&pat_path, serde_json::to_vec(pattern).unwrap()).unwrap();

        let real_dir = root.join("real_out");
        std::fs::create_dir(&real_dir).unwrap();
        let linked_dir = root.join("linked_out");
        symlink(&real_dir, &linked_dir).unwrap();
        let args = crate::parse(&[
            "export".into(),
            "svg".into(),
            pat_path.to_string_lossy().into_owned(),
            "--out".into(),
            linked_dir.to_string_lossy().into_owned(),
            "--created".into(),
            "2026-10-11T12:00".into(),
        ])
        .unwrap();
        let err = cmd_export(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("regular directory"), "{}", err.message);
        assert!(std::fs::read_dir(&real_dir).unwrap().next().is_none());

        let out_dir = root.join("out");
        std::fs::create_dir(&out_dir).unwrap();
        let victim = root.join("victim.svg");
        std::fs::write(&victim, b"untouched").unwrap();
        let parsed = Pattern::new(pattern.clone());
        let ctx = Ctx::new(&parsed, ExportOptions::default()).unwrap();
        let svg_link = out_dir.join(format!("{}_master_mm.svg", ctx.style));
        symlink(&victim, &svg_link).unwrap();
        let args = crate::parse(&[
            "export".into(),
            "svg".into(),
            pat_path.to_string_lossy().into_owned(),
            "--out".into(),
            out_dir.to_string_lossy().into_owned(),
            "--created".into(),
            "2026-10-11T12:00".into(),
        ])
        .unwrap();
        let err = cmd_export(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("regular file"), "{}", err.message);
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        assert!(
            !out_dir
                .join(format!("{}_export_manifest.json", ctx.style))
                .exists(),
            "manifest must not be written when another output file is a symlink"
        );

        let manifest_dir = root.join("manifest_link_out");
        std::fs::create_dir(&manifest_dir).unwrap();
        symlink(
            &victim,
            manifest_dir.join(format!("{}_export_manifest.json", ctx.style)),
        )
        .unwrap();
        let args = crate::parse(&[
            "export".into(),
            "svg".into(),
            pat_path.to_string_lossy().into_owned(),
            "--out".into(),
            manifest_dir.to_string_lossy().into_owned(),
            "--created".into(),
            "2026-10-11T12:00".into(),
        ])
        .unwrap();
        let err = cmd_export(&args).unwrap_err();
        assert_eq!(err.code, RefusalCode::InvalidDocument);
        assert!(err.message.contains("regular file"), "{}", err.message);
        assert_eq!(std::fs::read(&victim).unwrap(), b"untouched");
        assert!(
            !manifest_dir
                .join(format!("{}_master_mm.svg", ctx.style))
                .exists()
        );
    }
}
