//! `ftx export TARGET PATTERN --out DIR` and `ftx check DIR`: the reference's `export.py` (spec 03 §1.3) on
//! `ftx-export`.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use ftx_core::{Refusal, RefusalCode};
use ftx_export::{
    CheckItem, Ctx, ExportOptions, PyJson, Target, check_files, export_targets, read_dir_files,
};
use ftx_pattern::Pattern;
use serde_json::{Value, json};

use super::{Args, bad_input, known_options, make_dir, read_json};

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

fn load_pattern(path: &str) -> Result<Pattern, Refusal> {
    Ok(Pattern::new(read_json(path)?.0))
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
pub(super) fn cmd_check(
    args: &Args,
    dir: Option<&String>,
) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "check", &["pattern-ref"])?;
    let dir = dir.ok_or_else(|| bad_input("ftx check needs an export directory"))?;
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
        return cmd_check(args, args.positional.get(2));
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
        sa: number(args, "sa")?,
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
    let manifest_path = out.join(format!("{}_export_manifest.json", ctx.style));
    let existing = match std::fs::read_to_string(&manifest_path) {
        Ok(text) => Some(
            PyJson::parse(&text)
                .map_err(|e| bad_input(format!("{}: {e}", manifest_path.display())))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => {
            return Err(Refusal::new(
                RefusalCode::Io,
                format!("{}: {e}", manifest_path.display()),
            ));
        }
    };
    let set = export_targets(&ctx, &targets, existing)?;
    make_dir(&out)?;
    let mut written = Vec::new();
    for (rel, bytes) in &set.files {
        let path = out.join(rel);
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
    #[test]
    fn utc_minute_is_a_minute() {
        let s = super::utc_minute().unwrap();
        assert_eq!(s.len(), 16, "{s}");
        assert_eq!(&s[4..5], "-");
        assert_eq!(&s[10..11], "T");
        assert!(s.as_str() >= "2026-01-01T00:00");
    }
}
