//! Source-shell drafting through the shared numerical and machine-export authorities.

use std::path::PathBuf;

use ftx_core::Refusal;
use ftx_export::knit::{ShapedKnitSource, export_shaped_knit};
use ftx_pattern::knit::{AutoKnitConfig, DrapeUncertaintyReceipt, GaugeSpec};
use serde_json::{Value, json};

use super::{
    Args, bad_input, expected_knit_material, export, known_options, no_extra_positional,
    parse_machine, read_knit_qualification, read_optional_knit_json, read_utf8, reserve_output,
    write_file,
};

pub(super) fn cmd_knit_shape(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit shape",
        &[
            "config",
            "gauge",
            "machine",
            "qual",
            "material-sha256",
            "drape-receipt",
            "created",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        3,
        "ftx knit shape SHELL.json --config CONFIG.json --gauge GAUGE.json --out NEW_DIR [--machine ID] [--qual SWATCH.json --material-sha256 SHA]",
    )?;
    let out = PathBuf::from(crate::required(args, "out")?);
    let shell_json = read_utf8(&PathBuf::from(&args.positional[2]), "shaping shell")?;
    let config: AutoKnitConfig = read_optional_knit_json(args, "config", "shaping choices")?
        .ok_or_else(|| bad_input("`ftx knit shape` requires --config CONFIG.json"))?;
    let gauge: GaugeSpec = read_optional_knit_json(args, "gauge", "shaping gauge")?
        .ok_or_else(|| bad_input("`ftx knit shape` requires --gauge GAUGE.json"))?;
    let profile = parse_machine(
        args.options
            .get("machine")
            .map_or("hand-knit", String::as_str),
    )?;
    let qualification = read_knit_qualification(args)?;
    let receipt: Option<DrapeUncertaintyReceipt> =
        read_optional_knit_json(args, "drape-receipt", "shaped knit drape receipt")?;
    let created = args
        .options
        .get("created")
        .cloned()
        .map(Ok)
        .unwrap_or_else(export::utc_minute)?;
    let bundle = export_shaped_knit(
        ShapedKnitSource {
            shell_json: &shell_json,
            config: &config,
            gauge: &gauge,
        },
        &profile,
        qualification.as_ref(),
        expected_knit_material(args)?,
        receipt.as_ref(),
        &created,
    )?;
    let result = json!({
        "machine":profile.id,
        "source_file_sha256":bundle.source_file_sha256,
        "pattern_sha256":bundle.report.doc.canonical_sha256()?,
        "shape":bundle.report,
        "cert":bundle.export.compiled.cert,
        "vm_report":bundle.export.compiled.vm_report,
        "backend_export_refusal":bundle.export.backend_export_refusal,
        "kcode_verification":bundle.export.kcode_verification,
        "files":bundle.export.files.keys().collect::<Vec<_>>(),
        "out_dir":out,
        "hardware_execution_authorized":false,
        "physical_knitting_validated":false,
        "fit_claimed":false,
    });
    // Geometry, source admission, qualification and export checks finish before publication.
    reserve_output(&out, "knit shape")?;
    for (name, bytes) in &bundle.export.files {
        write_file(&out, name, bytes)?;
    }
    Ok((
        result,
        0,
        format!(
            "SHAPED KNIT: '{}' — {} courses, maximum girth residual {:.3} mm\nwrote {}",
            config.name,
            bundle.report.course_count,
            bundle.report.max_abs_girth_residual_mm,
            out.display(),
        ),
    ))
}
