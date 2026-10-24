//! Printable metric reference, spec 02 A.9. Pure Rust, local files, no image detector.
//! Pixel-edge geometry is shared by the PNG, lossless one-bit PDF and JSON sidecar.

use std::path::Path;

use ftx_core::{Refusal, RefusalCode};
use ftx_trace::calibration_sheet::{self, DEFAULT_DICTIONARY as DICTIONARY};
use serde_json::{Value, json};

use super::{no_files, numeric, option};
use crate::{Args, known_options, required, workspace};

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "measure calib-sheet",
        &["paper", "out", "square-mm", "marker-mm", "dictionary"],
    )?;
    no_files(args, "calib-sheet")?;
    let paper = option(args, "paper").unwrap_or("letter");
    let out = Path::new(required(args, "out")?);
    let dictionary = option(args, "dictionary").unwrap_or(DICTIONARY);
    let rendered = calibration_sheet::render(
        paper,
        numeric(args, "square-mm")?.unwrap_or(22.0),
        numeric(args, "marker-mm")?.unwrap_or(16.0),
        dictionary,
    )?;
    let sidecar = rendered.sidecar_json();
    let mut result = rendered.sidecar;
    let files = [
        (format!("calib-sheet-{paper}.png"), rendered.png),
        (format!("calib-sheet-{paper}.pdf"), rendered.pdf),
        (format!("calib-sheet-{paper}.json"), sidecar),
    ];
    // Complete computation first. Exclusively claim a new directory so an existing sheet,
    // evidence file or filesystem alias can never be replaced. The sidecar is written last.
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| Refusal::new(RefusalCode::Io, e.to_string()))?;
    }
    std::fs::create_dir(out).map_err(|e| {
        Refusal::new(RefusalCode::Io, format!("{}: {e}", out.display()))
            .repair("choose a new output directory; existing calibration sheets are never replaced")
    })?;
    let mut paths = serde_json::Map::new();
    for (name, bytes) in files {
        let path = out.join(name);
        workspace::write_new(&path, &bytes)?;
        paths.insert(
            path.extension().unwrap().to_string_lossy().into_owned(),
            json!(path),
        );
    }
    result["paths"] = Value::Object(paths);
    Ok((
        result,
        0,
        format!(
            "Calibration sheet: {} (300 DPI, {paper}, {dictionary})\nPrint at 100% / Actual size. Check both bars with a ruler before use.",
            out.display()
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(out: &Path, extra: &[&str]) -> Args {
        let mut argv = vec![
            "measure".into(),
            "calib-sheet".into(),
            "--out".into(),
            out.to_str().unwrap().into(),
            "--json".into(),
        ];
        argv.extend(extra.iter().map(|s| s.to_string()));
        crate::parse(&argv).unwrap()
    }

    fn scratch() -> std::path::PathBuf {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "ftx-calibration-{}-{nanos}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ))
    }

    #[test]
    fn cli_publishes_all_three_files_and_never_overwrites_a_previous_sheet() {
        let out = scratch();
        let (result, code, text) = super::run(&args(&out, &[])).unwrap();
        assert_eq!(code, 0);
        assert!(text.contains("100% / Actual size"));
        let names = [
            "calib-sheet-letter.png",
            "calib-sheet-letter.pdf",
            "calib-sheet-letter.json",
        ];
        let before: Vec<_> = names
            .iter()
            .map(|name| std::fs::read(out.join(name)).unwrap())
            .collect();
        let saved: Value = serde_json::from_slice(&before[2]).unwrap();
        assert!(saved.get("paths").is_none());
        let mut result = result;
        assert_eq!(result["paths"]["png"], json!(out.join(names[0])));
        result.as_object_mut().unwrap().remove("paths");
        assert_eq!(result, saved);
        assert!(super::run(&args(&out, &[])).is_err());
        for (name, bytes) in names.iter().zip(before) {
            assert_eq!(std::fs::read(out.join(name)).unwrap(), bytes);
        }
    }

    #[test]
    fn cli_refuses_invalid_requests_before_creating_output_or_parent() {
        for extra in [
            vec!["--paper", "legal"],
            vec!["--square-mm", "22.1"],
            vec!["--marker-mm", "22"],
            vec!["--dictionary", "misspelled"],
            vec!["--sqaure-mm", "22"],
        ] {
            let parent = scratch();
            assert!(super::run(&args(&parent.join("sheet"), &extra)).is_err());
            assert!(!parent.exists());
        }
    }
}
