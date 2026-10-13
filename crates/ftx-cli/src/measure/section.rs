//! Section-derived girths connect calibrated photo widths/depths to measurement fusion.

use ftx_core::Refusal;
use ftx_measure::{
    SectionOptions, calibrate_k, canonical_name, definition, fuse, girth_from_section, make_record,
};
use serde_json::{Value, json};

use super::{no_files, numeric, option};
use crate::{Args, bad_input, known_options, read_json, required};

fn photo_value(data: &Value, name: &str) -> Result<(f64, f64), Refusal> {
    let entry = &data["fused"][name];
    let v = entry["value_mm"]
        .as_f64()
        .ok_or_else(|| bad_input(format!("no {name} record")))?;
    let sigma = entry["sigma_mm"]
        .as_f64()
        .ok_or_else(|| bad_input(format!("{name} lacks uncertainty")))?;
    let sources = entry["sources"]
        .as_array()
        .ok_or_else(|| bad_input(format!("{name} lacks provenance")))?;
    if entry["flags"]
        .as_array()
        .is_some_and(|flags| flags.iter().any(|f| f == "REMEASURE"))
    {
        return Err(bad_input(format!(
            "{name}: conflicting evidence must be remeasured before deriving a girth"
        )));
    }
    let used: Vec<_> = sources.iter().filter(|s| s["used"] == true).collect();
    if used.is_empty() || used.iter().any(|s| s["provenance"] != "PHOTO_ESTIMATED") {
        return Err(bad_input(format!(
            "{name}: --from requires photo evidence, not chart/model values or a mixed-provenance estimate"
        )));
    }
    Ok((v, sigma))
}

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
    let source = option(args, "from");
    let evidence = if let Some(path) = source {
        for key in ["width-mm", "depth-mm", "sigma-width-mm", "sigma-depth-mm"] {
            if option(args, key).is_some() {
                return Err(bad_input(format!(
                    "--{key} cannot override --from evidence"
                )));
            }
        }
        Some(fuse(&read_json(path)?.0, false)?)
    } else {
        None
    };
    let (width, sw, depth, sd) = if let Some(data) = &evidence {
        let (w, sw) = photo_value(data, &format!("{level}_width"))?;
        let (d, sd) = photo_value(data, &format!("{level}_depth"))?;
        (w, sw, d, sd)
    } else {
        required(args, "width-mm")?;
        required(args, "depth-mm")?;
        (
            numeric(args, "width-mm")?.unwrap(),
            numeric(args, "sigma-width-mm")?.unwrap_or(0.0),
            numeric(args, "depth-mm")?.unwrap(),
            numeric(args, "sigma-depth-mm")?.unwrap_or(0.0),
        )
    };
    let options = SectionOptions {
        level,
        sigma_width_mm: sw,
        sigma_depth_mm: sd,
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
    let mut out = girth_from_section(width, depth, &options)?;
    let name = option(args, "name")
        .or_else(|| ["chest", "waist", "hip"].contains(&level).then_some(level));
    if let Some(name) = name {
        let name = canonical_name(name);
        if definition(&name).is_some_and(|d| d["kind"] != "girth") || name.ends_with("_deg") {
            return Err(bad_input(
                "the derived record name must identify a girth, not a width, length or angle",
            ));
        }
        let method = format!(
            "girth-from-section k·Ramanujan(width {width}, depth {depth}), k={}",
            out["k"]
        );
        let record = make_record(
            &name,
            out["girth_mm"].as_f64().unwrap(),
            out["sigma_mm"].as_f64().unwrap(),
            "PHOTO_ESTIMATED",
            &method,
            None,
            out["notes"].as_str().unwrap(),
        )?;
        // Keep the reference's `record` field, plus a schema-valid document for direct fusion.
        out["record"] = record.clone();
        out["schema"] = json!("fabric-measurements/1");
        out["wearer"] = evidence.as_ref().map_or(json!({}), |d| d["wearer"].clone());
        out["taken_on"] = evidence
            .as_ref()
            .map_or(Value::Null, |d| d["taken_on"].clone());
        out["records"] = json!([record]);
        out["fused"] = json!({});
        if let Some(data) = evidence {
            out["section_inputs"] = json!({"source":source,
                "width":data["fused"][format!("{level}_width")], "depth":data["fused"][format!("{level}_depth")]});
        }
    }
    Ok(out)
}
