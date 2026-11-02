//! Section-derived girths connect calibrated photo widths/depths to measurement fusion.

use ftx_core::Refusal;
use ftx_measure::{SectionOptions, calibrate_k, photo_section, section_measurement};
use serde_json::{Value, json};
use std::path::Path;

use super::{flag, no_files, numeric, option};
use crate::{Args, bad_input, known_options, required};

pub(super) fn run(args: &Args) -> Result<Value, Refusal> {
    known_options(
        args,
        "girth-from-section",
        &[
            "level",
            "width-mm",
            "depth-mm",
            "sigma-width-mm",
            "sigma-depth-mm",
            "from",
            "depth-from",
            "same-wearer",
            "k",
            "k-sigma",
            "k-ratio",
            "k-ratio-sigma",
            "clothing-mm",
            "tape-girth-mm",
            "sigma-tape-mm",
            "name",
            "out",
        ],
    )?;
    no_files(args, "girth-from-section")?;
    let level = option(args, "level").unwrap_or("waist");
    if let Some(path) = option(args, "from") {
        for key in ["width-mm", "depth-mm", "sigma-width-mm", "sigma-depth-mm"] {
            if option(args, key).is_some() {
                return Err(bad_input(format!(
                    "--{key} cannot override --from evidence"
                )));
            }
        }
        let read = |path: &str| {
            let bytes = super::video::read_limited(Path::new(path), 16 * 1024 * 1024)?;
            String::from_utf8(bytes).map_err(|e| bad_input(format!("{path}: {e}")))
        };
        let width = read(path)?;
        let depth = option(args, "depth-from")
            .map(read)
            .transpose()?
            .unwrap_or_default();
        let mut options = json!({"level":level,"same_wearer":flag(args,"same-wearer")});
        for (flag, key) in [
            ("k", "k"),
            ("k-sigma", "k_sigma"),
            ("k-ratio", "k_ratio"),
            ("k-ratio-sigma", "k_ratio_sigma"),
            ("clothing-mm", "clothing_mm"),
            ("tape-girth-mm", "tape_girth_mm"),
            ("sigma-tape-mm", "sigma_tape_mm"),
        ] {
            if let Some(value) = numeric(args, flag)? {
                options[key] = json!(value);
            }
        }
        if let Some(name) = option(args, "name") {
            options["name"] = json!(name);
        }
        let mut result = photo_section(&width, &depth, &options.to_string())?;
        result["section_inputs"]["source"] = json!(path);
        if let Some(path) = option(args, "depth-from") {
            result["section_inputs"]["depth_source"] = json!(path);
        }
        return Ok(result);
    }
    if option(args, "depth-from").is_some() || flag(args, "same-wearer") {
        return Err(bad_input("--depth-from and --same-wearer require --from"));
    }
    required(args, "width-mm")?;
    required(args, "depth-mm")?;
    let width = numeric(args, "width-mm")?.unwrap();
    let depth = numeric(args, "depth-mm")?.unwrap();
    let options = SectionOptions {
        level,
        sigma_width_mm: numeric(args, "sigma-width-mm")?.unwrap_or(0.0),
        sigma_depth_mm: numeric(args, "sigma-depth-mm")?.unwrap_or(0.0),
        k: numeric(args, "k")?,
        k_sigma: numeric(args, "k-sigma")?,
        k_ratio: numeric(args, "k-ratio")?.unwrap_or(1.0),
        k_ratio_sigma: numeric(args, "k-ratio-sigma")?.unwrap_or(0.0),
        clothing_mm: numeric(args, "clothing-mm")?.unwrap_or(0.0),
    };
    if let Some(tape) = numeric(args, "tape-girth-mm")? {
        for key in ["k", "k-sigma", "k-ratio", "k-ratio-sigma", "name"] {
            if option(args, key).is_some() {
                return Err(bad_input(format!(
                    "--{key} does not apply when calibrating --tape-girth-mm"
                )));
            }
        }
        return calibrate_k(
            tape,
            width,
            depth,
            numeric(args, "sigma-tape-mm")?.unwrap_or(10.0),
            &options,
        );
    }
    if option(args, "sigma-tape-mm").is_some() {
        return Err(bad_input("--sigma-tape-mm requires --tape-girth-mm"));
    }
    section_measurement(width, depth, &options, option(args, "name"))
}
