//! Local decoded-image boundary for FTX-20; no decoder process, model download or network access.

use std::collections::BTreeSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use ftx_core::{Refusal, RefusalCode, receipt::sha256_hex};
use ftx_measure::video::{FrameMetadata, SelectionOptions, score_frame, select_frames};
use ftx_trace::raster::{
    color::bgr_to_gray,
    image::{Gray, load_png_with_limit},
    jpeg::decode_photo,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Args, bad_input, known_options};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FrameFiles {
    metadata: FrameMetadata,
    image: String,
    person_mask: String,
    sheet_mask: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    schema: String,
    decode_receipt: String,
    #[serde(default)]
    options: SelectionOptions,
    frames: Vec<FrameFiles>,
}

fn io(path: &Path, error: impl std::fmt::Display) -> Refusal {
    Refusal::new(RefusalCode::Io, format!("{}: {error}", path.display()))
}

pub(super) fn read_limited(path: &Path, limit: usize) -> Result<Vec<u8>, Refusal> {
    let metadata = std::fs::metadata(path).map_err(|e| io(path, e))?;
    if !metadata.is_file() || metadata.len() > limit as u64 {
        return Err(bad_input(format!(
            "{} must be a regular file of at most {limit} bytes",
            path.display()
        )));
    }
    let file = File::open(path).map_err(|e| io(path, e))?;
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| io(path, e))?;
    if bytes.len() > limit {
        return Err(bad_input(format!(
            "{} exceeds the byte limit",
            path.display()
        )));
    }
    Ok(bytes)
}

pub(super) fn load(base: &Path, name: &str, mask: bool) -> Result<Gray, Refusal> {
    load_hashed(base, name, mask).map(|(gray, _)| gray)
}

pub(super) fn load_hashed(base: &Path, name: &str, mask: bool) -> Result<(Gray, String), Refusal> {
    if name.trim().is_empty() || name.len() > 4096 {
        return Err(bad_input(
            "image and mask paths must be nonempty and at most 4096 bytes",
        ));
    }
    let path = base.join(name);
    let bytes = read_limited(&path, 128 * 1024 * 1024)?;
    if !mask && bytes.starts_with(&[0xff, 0xd8]) {
        let photo = decode_photo(&bytes, 40_000_000)?;
        // Frame metadata, both masks and calibrated pixels already share one orientation. A
        // photo's automatic EXIF transpose would change only the frame and invalidate that contract.
        if photo
            .exif_orientation
            .is_some_and(|orientation| orientation != 1)
        {
            return Err(bad_input(format!(
                "{}: video frames must already be oriented; apply JPEG EXIF orientation to pixels, both masks and calibration before selection",
                path.display()
            )));
        }
        return Ok((bgr_to_gray(&photo.image), sha256_hex(&bytes)));
    }
    // Restrict the evidence boundary to explicit 8-bit pixels. In particular, alpha compositing
    // on white would silently turn transparent mask backgrounds into foreground evidence.
    if bytes.len() < 33
        || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || &bytes[8..16] != b"\0\0\0\rIHDR"
        || bytes[24] != 8
        || if mask {
            bytes[25] != 0
        } else {
            !matches!(bytes[25], 0 | 2)
        }
    {
        return Err(bad_input(format!(
            "{}: use an opaque 8-bit {} PNG with orientation already applied",
            path.display(),
            if mask {
                "grayscale mask"
            } else {
                "grayscale or RGB frame"
            }
        )));
    }
    let mut pos = 8usize;
    let mut ended = false;
    while pos + 12 <= bytes.len() {
        let len = u32::from_be_bytes(bytes[pos..pos + 4].try_into().unwrap()) as usize;
        let end = pos
            .checked_add(12)
            .and_then(|p| p.checked_add(len))
            .filter(|&p| p <= bytes.len())
            .ok_or_else(|| bad_input("truncated PNG chunk"))?;
        if matches!(
            &bytes[pos + 4..pos + 8],
            b"tRNS" | b"eXIf" | b"acTL" | b"fcTL" | b"fdAT"
        ) {
            return Err(bad_input(format!(
                "{}: remove transparency, animation and EXIF metadata after decoding and orienting the pixels",
                path.display()
            )));
        }
        if &bytes[pos + 4..pos + 8] == b"IEND" {
            if len != 0 || end != bytes.len() {
                return Err(bad_input(
                    "PNG must end with an empty IEND and no trailing data",
                ));
            }
            ended = true;
        }
        pos = end;
    }
    if !ended {
        return Err(bad_input("PNG has no complete IEND"));
    }
    let image = load_png_with_limit(&bytes, 40_000_000)
        .map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    let gray = bgr_to_gray(&image);
    if mask && gray.data.iter().any(|&v| v != 0 && v != 255) {
        return Err(bad_input(format!(
            "{}: masks must contain only 0 (outside) and 255 (inside)",
            path.display()
        )));
    }
    Ok((gray, sha256_hex(&bytes)))
}

pub(super) fn run(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "measure video-select", &["out"])?;
    let path = Path::new(super::single(args, "video-select")?);
    let result = analyze(path)?;
    if let Some(output) = super::option(args, "out") {
        super::photo::save(output, &result.0)?;
        eprintln!("wrote {output}");
    }
    Ok(result)
}

pub(super) fn analyze(path: &Path) -> Result<(Value, i32, String), Refusal> {
    let bytes = read_limited(path, 16 * 1024 * 1024)?;
    let mut input: Manifest = serde_json::from_slice(&bytes)
        .map_err(|e| bad_input(format!("{}: {e}", path.display())))?;
    if input.schema != "ftx-video-frames/1"
        || input.decode_receipt.trim().is_empty()
        || input.decode_receipt.len() > 4096
        || input.frames.is_empty()
        || input.frames.len() > 20_000
    {
        return Err(bad_input(
            "video selection needs schema ftx-video-frames/1, a decode receipt and 1..20000 frames",
        ));
    }
    input.options.validate()?;
    input
        .frames
        .sort_by(|a, b| a.metadata.frame.cmp(&b.metadata.frame));
    if input
        .frames
        .windows(2)
        .any(|p| p[0].metadata.frame == p[1].metadata.frame)
    {
        return Err(bad_input("duplicate decoded stream/timestamp identity"));
    }
    let source = std::fs::canonicalize(path).map_err(|e| io(path, e))?;
    let base = source
        .parent()
        .ok_or_else(|| bad_input("manifest needs a parent directory"))?;
    let mut scores = Vec::with_capacity(input.frames.len());
    let mut sources = Vec::with_capacity(input.frames.len());
    for frame in &input.frames {
        let (image, image_sha) = load_hashed(base, &frame.image, false)?;
        let (person, person_sha) = load_hashed(base, &frame.person_mask, true)?;
        let (sheet, sheet_sha) = load_hashed(base, &frame.sheet_mask, true)?;
        if [image.w, image.h] != [person.w, person.h] || [image.w, image.h] != [sheet.w, sheet.h] {
            return Err(bad_input(format!(
                "{}: both masks must have the oriented frame's exact dimensions; no resampling is performed",
                frame.image
            )));
        }
        scores.push(score_frame(
            frame.metadata.clone(),
            [image.w, image.h],
            &image.data,
            &person.data,
            &sheet.data,
        )?);
        sources.push((
            frame.metadata.frame.clone(),
            json!({"metadata":frame.metadata,"image":frame.image,
            "person_mask":frame.person_mask,"sheet_mask":frame.sheet_mask,
            "image_size_px":[image.w,image.h],
            "image_sha256":image_sha,"person_mask_file_sha256":person_sha,
            "sheet_mask_file_sha256":sheet_sha,
            "decoded_gray_sha256":sha256_hex(&image.data),
            "person_mask_sha256":sha256_hex(&person.data),
            "sheet_mask_sha256":sha256_hex(&sheet.data)}),
        ));
    }
    let report = select_frames(&scores, &input.options)?;
    let missing = report
        .windows
        .iter()
        .filter(|w| w.selected.is_none())
        .count();
    let code = if missing == 0 { 0 } else { 2 };
    let status = if missing == 0 {
        "SELECTED"
    } else if report.selected_frames.is_empty() {
        "NO_USABLE_FRAMES"
    } else {
        "PARTIAL"
    };
    let mut text = format!(
        "VIDEO {status}: selected {} of {} decoded frames across {} populated windows; {missing} windows have no eligible frame.\n{}",
        report.selected_frames.len(),
        input.frames.len(),
        report.windows.len(),
        report.no_claim
    );
    for advisory in &report.advisories {
        text.push_str(&format!("\n{advisory}"));
    }
    let mut value = serde_json::to_value(&report).map_err(|e| bad_input(e.to_string()))?;
    value["status"] = json!(status);
    value["input"] = serde_json::to_value(input).map_err(|e| bad_input(e.to_string()))?;
    value["source_manifest"] = json!(source.to_string_lossy());
    let selected: BTreeSet<_> = report.selected_frames.iter().collect();
    value["selected_sources"] = Value::Array(
        sources
            .into_iter()
            .filter(|(id, _)| selected.contains(id))
            .map(|(_, source)| source)
            .collect(),
    );
    Ok((value, code, text))
}
