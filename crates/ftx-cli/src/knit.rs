//! `ftx knit` and `ftx dye` subcommands (`FTX-K17`):
//! - `ftx knit swatch` — hard-capped `CalibrationSwatchOnly` kit (`.k`, `.kc`/AYAB, CYC SVG/PDF, cert, template)
//! - `ftx knit draft` — measured fully-fashioned panel or top-down seamless raglan with child-safety gates
//! - `ftx knit verify` — 7-check V-bed VM & child-safety verifier for any `ftx-knit/1` document
//! - `ftx knit poincloux` — Rung-3 Poincloux et al. (2018) planar elastica analytical KAT curve
//! - `ftx dye optimize` — 4-stage radiometric KM + Sobol-CVaR Bertsekas L-BFGS + 5-regime Pareto
//! - `ftx dye verify` — physical dye-rig Capstan/dwell/Tg/accumulator/fastness verifier + `.csv`/`.gcode`/cert

use std::path::PathBuf;

use ftx_core::{Refusal, RefusalCode, TargetRaster};
use ftx_drape::dye::{
    CapillaryTransportConfig, DriftParameters, DyeInverseConfig, DyeOpticsModel, KnitStitchMap,
    optimize_yarn_dye_program, rank_five_regime_pareto, srgb_u8_to_xyz_d65,
};
use ftx_drape::knit::{PoinclouxParams, poincloux_uniaxial_curve};
use ftx_export::dye::verify_and_export_dye_program;
use ftx_export::knit::{
    check_hand_knit_pdf, check_hand_knit_svg, export_brother_ayab, export_calibration_swatch_kit,
    export_hand_knit_document, export_kniterate_kc, export_knitout_v2,
    refuse_proprietary_binary_target,
};
use ftx_pattern::dye::{DyeMachineProfile, DyeingRegime, YarnDyeProgram, YarnDyeingQualification};
use ftx_pattern::knit::{
    CarrierId, GaugeSpec, KnitDoc, KnitSwatchQualification, MachineBackendKind, MachineProfile,
    ParametricPanelSpec, RaglanPulloverSpec, WearerAgeClass, compile_knit_doc,
    draft_seamless_raglan_tube, draft_shaped_panel,
};
use ftx_trace::raster::load_target_raster_with_limit;
use serde_json::{Value, json};

use super::{Args, bad_input, export, reserve_output, workspace};

/// Boolean flags accepted by `ftx knit` and `ftx dye` without a following value.
pub(super) const BOOLEAN_OPTIONS: &[&str] =
    &["circular", "has-drawstring", "allow-proprietary-binary"];

fn parse_machine(id: &str) -> Result<MachineProfile, Refusal> {
    match id {
        "kniterate-7e" | "kniterate" => Ok(MachineProfile::kniterate_7e()),
        "brother-kh930-ayab" | "brother-kh930" | "kh930" => {
            Ok(MachineProfile::brother_kh930_ayab())
        }
        "brother-kh270-ayab" | "brother-kh270" | "kh270" => {
            Ok(MachineProfile::brother_kh270_ayab())
        }
        "shima-seiki-12e-handoff" | "shima-12e" | "shima" => {
            Ok(MachineProfile::shima_seiki_12e_handoff())
        }
        "hand-knit" | "hand" => Ok(MachineProfile::hand_knit(4.0)),
        _ => Err(bad_input(format!(
            "unknown --machine '{id}' (expected kniterate-7e, brother-kh930, brother-kh270, shima-12e, or hand-knit)"
        ))),
    }
}

fn parse_dye_machine(id: &str) -> Result<DyeMachineProfile, Refusal> {
    match id {
        "piezo-cmyk" | "benchtop-piezo-cmyk-v1" => Ok(DyeMachineProfile::benchtop_piezo_cmyk()),
        "dip-3bath" | "multi-bath-dip-v1" => Ok(DyeMachineProfile::multi_bath_dip_rig()),
        _ => Err(bad_input(format!(
            "unknown --machine '{id}' (expected piezo-cmyk or dip-3bath)"
        ))),
    }
}

fn parse_age_class(raw: &str) -> Result<WearerAgeClass, Refusal> {
    match raw {
        "infant" | "infant_under3" => Ok(WearerAgeClass::InfantUnder3),
        "child" | "child_3_to_14" => Ok(WearerAgeClass::Child3To14),
        "adult" => Ok(WearerAgeClass::Adult),
        "swatch" | "calibration_swatch_only" => Ok(WearerAgeClass::CalibrationSwatchOnly),
        _ => Err(bad_input(format!(
            "invalid --age-class '{raw}' (expected infant, child, adult, or swatch)"
        ))),
    }
}

fn parse_regime(raw: &str) -> Result<DyeingRegime, Refusal> {
    match raw {
        "stripes" | "1a" => Ok(DyeingRegime::Regime1AStripes),
        "pooling" | "1b" => Ok(DyeingRegime::Regime1BCircularPooling),
        "fiducial" | "2" => Ok(DyeingRegime::Regime2FiducialSynced),
        "inline" | "3" => Ok(DyeingRegime::Regime3InlineDancer),
        "jacquard" | "4" => Ok(DyeingRegime::Regime4MultiCarrierJacquard),
        _ => Err(bad_input(format!(
            "invalid --regime '{raw}' (expected stripes, pooling, fiducial, inline, or jacquard)"
        ))),
    }
}

fn opt_f64(args: &Args, key: &str, default: f64) -> Result<f64, Refusal> {
    match args.options.get(key) {
        Some(v) => v
            .parse::<f64>()
            .map_err(|_| bad_input(format!("--{key} must be a number, got '{v}'"))),
        None => Ok(default),
    }
}

fn opt_u32(args: &Args, key: &str, default: u32) -> Result<u32, Refusal> {
    match args.options.get(key) {
        Some(v) => v
            .parse::<u32>()
            .map_err(|_| bad_input(format!("--{key} must be an integer, got '{v}'"))),
        None => Ok(default),
    }
}

fn write_file(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<(), Refusal> {
    let p = dir.join(name);
    std::fs::write(&p, bytes).map_err(|e| workspace::io_error(&p, e))
}

/// Dispatches `ftx knit <swatch|draft|verify|poincloux> ...`.
pub(super) fn cmd_knit(args: &Args) -> Result<(Value, i32, String), Refusal> {
    match args.positional.get(1).map(String::as_str) {
        Some("swatch") => cmd_knit_swatch(args),
        Some("draft") => cmd_knit_draft(args),
        Some("verify") => cmd_knit_verify(args),
        Some("poincloux") => cmd_knit_poincloux(args),
        _ => Err(bad_input(
            "expected `ftx knit swatch|draft|verify|poincloux`",
        )),
    }
}

fn cmd_knit_swatch(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("kniterate-7e");
    let profile = parse_machine(machine_id)?;
    let yarn_id = args
        .options
        .get("yarn")
        .map(String::as_str)
        .unwrap_or("merino-200tex");
    let needles = opt_u32(args, "needles", 40)?;
    let courses_per_zone = opt_u32(args, "courses-per-zone", 20)?;
    let cams: Vec<f64> = match args.options.get("cams") {
        Some(s) => s
            .split(',')
            .map(|tok| {
                tok.trim()
                    .parse::<f64>()
                    .map_err(|_| bad_input(format!("invalid cam setting '{tok}'")))
            })
            .collect::<Result<Vec<_>, _>>()?,
        None => vec![
            profile.cam_setting_range[0]
                + 0.35 * (profile.cam_setting_range[1] - profile.cam_setting_range[0]),
            profile.cam_setting_range[0]
                + 0.55 * (profile.cam_setting_range[1] - profile.cam_setting_range[0]),
        ],
    };
    let created = match args.options.get("created") {
        Some(c) => c.clone(),
        None => export::utc_minute()?,
    };
    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit swatch` requires --out NEW_DIR"))?,
    );

    let kit = export_calibration_swatch_kit(
        &profile,
        yarn_id,
        "unmeasured-yarn-lot-placeholder",
        needles,
        courses_per_zone,
        &cams,
    )?;
    let compiled = compile_knit_doc(&kit.doc, &profile, None, None, None)?;
    let hand = export_hand_knit_document(&kit.doc, &created)?;
    let svg_checks = check_hand_knit_svg(&hand.svg_text, "swatch_chart.svg");
    let pdf_checks = check_hand_knit_pdf(&hand.pdf_bytes, "swatch_chart.pdf");
    if !svg_checks.iter().all(|c| c.ok) || !pdf_checks.iter().all(|c| c.ok) {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "hand-knit chart acceptance check failed",
        ));
    }

    reserve_output(&out_dir, "knit swatch")?;
    let mut files = vec![
        "swatch.knit.json".to_string(),
        "swatch.k".to_string(),
        "cert.json".to_string(),
        "qualification_template.json".to_string(),
        "swatch_chart.svg".to_string(),
        "swatch_chart.pdf".to_string(),
        "swatch_instructions.txt".to_string(),
    ];
    let doc_json = serde_json::to_string_pretty(&kit.doc)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    write_file(&out_dir, "swatch.knit.json", doc_json.as_bytes())?;
    write_file(&out_dir, "swatch.k", kit.knitout.knitout_text.as_bytes())?;
    write_file(&out_dir, "cert.json", kit.knitout.cert_json.as_bytes())?;
    write_file(
        &out_dir,
        "qualification_template.json",
        kit.qualification_template_json.as_bytes(),
    )?;
    write_file(&out_dir, "swatch_chart.svg", hand.svg_text.as_bytes())?;
    write_file(&out_dir, "swatch_chart.pdf", &hand.pdf_bytes)?;
    write_file(
        &out_dir,
        "swatch_instructions.txt",
        hand.instructions_text.as_bytes(),
    )?;

    if profile.backend_kind == MachineBackendKind::KniterateKc {
        let kc = export_kniterate_kc(&compiled, &profile)?;
        write_file(&out_dir, "swatch.kc", kc.kc_text.as_bytes())?;
        files.push("swatch.kc".to_string());
    } else if profile.backend_kind == MachineBackendKind::BrotherAyab {
        let ayab = export_brother_ayab(&kit.doc, &profile, &compiled.cert)?;
        write_file(&out_dir, "swatch_ayab.png", &ayab.png_bytes)?;
        write_file(
            &out_dir,
            "ayab_session_checklist.json",
            ayab.checklist_json.as_bytes(),
        )?;
        files.push("swatch_ayab.png".to_string());
        files.push("ayab_session_checklist.json".to_string());
    }

    let result = json!({
        "machine": profile.id,
        "wearer_age_class": "calibration_swatch_only",
        "out_dir": out_dir.display().to_string(),
        "files": files,
        "knitout_sha256": kit.knitout.knitout_sha256,
        "cert_sha256": kit.knitout.cert.cert_sha256,
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
    });
    let text = format!(
        "Generated CalibrationSwatchOnly kit for {} in {} ({} files; hardware_execution_authorized=false)",
        profile.id,
        out_dir.display(),
        files.len()
    );
    Ok((result, 0, text))
}

fn cmd_knit_draft(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let style = args
        .options
        .get("style")
        .map(String::as_str)
        .unwrap_or("panel");
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("kniterate-7e");
    let profile = parse_machine(machine_id)?;
    if args.options.contains_key("allow-proprietary-binary") {
        refuse_proprietary_binary_target(&profile, "proprietary_binary")?;
    }
    let age_str = args.options.get("age-class").ok_or_else(|| {
        bad_input("`ftx knit draft` requires explicit --age-class infant|child|adult")
    })?;
    let wearer_age_class = parse_age_class(age_str)?;
    let wales_per_100mm = opt_f64(args, "wales", 24.0)?;
    let courses_per_100mm = opt_f64(args, "courses", 32.0)?;
    let gauge = GaugeSpec {
        wales_per_100mm,
        courses_per_100mm,
        needle_pitch_mm: profile.needle_pitch_mm,
        yarn_tex: args.options.get("yarn-tex").and_then(|s| s.parse().ok()),
        thickness_mm: Some(opt_f64(args, "thickness-mm", 2.0)?),
        relaxed_after_wash: true,
    };
    let yarn_id = args
        .options
        .get("yarn")
        .cloned()
        .unwrap_or_else(|| "merino-180tex".to_string());
    let cam = opt_f64(
        args,
        "cam",
        0.5 * (profile.cam_setting_range[0] + profile.cam_setting_range[1]),
    )?;
    let has_drawstring = args.options.contains_key("has-drawstring");

    let doc = match style {
        "panel" => draft_shaped_panel(
            &ParametricPanelSpec {
                name: args
                    .options
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| "knitted_panel".to_string()),
                wearer_age_class,
                hem_width_mm: opt_f64(args, "width-mm", 260.0)?,
                chest_width_mm: opt_f64(args, "chest-mm", 280.0)?,
                length_mm: opt_f64(args, "length-mm", 320.0)?,
                armhole_depth_mm: opt_f64(args, "armhole-mm", 110.0)?,
                armhole_inset_total_mm: opt_f64(args, "armhole-inset-mm", 35.0)?,
                neck_width_mm: opt_f64(args, "neck-mm", 100.0)?,
                neck_depth_mm: opt_f64(args, "neck-depth-mm", 35.0)?,
                rib_hem_courses: opt_u32(args, "rib-courses", 8)?,
                cam_setting: cam,
                yarn_id,
                carrier_id: profile.carrier_ids[0].clone(),
                has_hood_or_neck_drawstring: has_drawstring,
            },
            &gauge,
            &profile,
        )?,
        "raglan" => draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: args
                    .options
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| "raglan_pullover".to_string()),
                wearer_age_class,
                neck_girth_mm: opt_f64(args, "neck-mm", 280.0)?,
                chest_girth_mm: opt_f64(args, "chest-mm", 560.0)?,
                yoke_depth_mm: opt_f64(args, "yoke-mm", 120.0)?,
                body_length_below_arm_mm: opt_f64(args, "length-mm", 200.0)?,
                collar_rib_courses: opt_u32(args, "collar-rib", 6)?,
                hem_rib_courses: opt_u32(args, "rib-courses", 8)?,
                cam_setting: cam,
                yarn_id,
                carrier_id: profile.carrier_ids[0].clone(),
                has_hood_or_neck_drawstring: has_drawstring,
            },
            &gauge,
            &profile,
        )?,
        _ => {
            return Err(bad_input(format!(
                "unknown --style '{style}' (expected panel or raglan)"
            )));
        }
    };

    let qual: Option<KnitSwatchQualification> = match args.options.get("qual") {
        Some(path) => {
            let p = PathBuf::from(path);
            let txt = std::fs::read_to_string(&p).map_err(|e| workspace::io_error(&p, e))?;
            Some(
                serde_json::from_str(&txt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };
    let compiled = compile_knit_doc(&doc, &profile, qual.as_ref(), None, None)?;
    let knitout = export_knitout_v2(&compiled)?;
    let created = match args.options.get("created") {
        Some(c) => c.clone(),
        None => export::utc_minute()?,
    };
    let hand = export_hand_knit_document(&doc, &created)?;

    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit draft` requires --out NEW_DIR"))?,
    );
    reserve_output(&out_dir, "knit draft")?;

    let doc_json = serde_json::to_string_pretty(&doc)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    write_file(&out_dir, "pattern.knit.json", doc_json.as_bytes())?;
    write_file(&out_dir, "program.k", knitout.knitout_text.as_bytes())?;
    write_file(&out_dir, "cert.json", knitout.cert_json.as_bytes())?;
    write_file(&out_dir, "chart.svg", hand.svg_text.as_bytes())?;
    write_file(&out_dir, "chart.pdf", &hand.pdf_bytes)?;
    write_file(
        &out_dir,
        "instructions.txt",
        hand.instructions_text.as_bytes(),
    )?;

    if profile.backend_kind == MachineBackendKind::KniterateKc {
        let kc = export_kniterate_kc(&compiled, &profile)?;
        write_file(&out_dir, "program.kc", kc.kc_text.as_bytes())?;
    } else if profile.backend_kind == MachineBackendKind::BrotherAyab {
        let ayab = export_brother_ayab(&doc, &profile, &compiled.cert)?;
        write_file(&out_dir, "pattern_ayab.png", &ayab.png_bytes)?;
        write_file(
            &out_dir,
            "ayab_session_checklist.json",
            ayab.checklist_json.as_bytes(),
        )?;
    }

    let result = json!({
        "style": style,
        "machine": profile.id,
        "stitches": doc.stitches.len(),
        "transfers": compiled.vm_report.total_transfers,
        "max_float_mm": compiled.vm_report.max_float_mm,
        "metadata": doc.metadata,
        "cert": compiled.cert,
        "out_dir": out_dir.display().to_string(),
    });
    let text = format!(
        "Drafted & verified {style} knit ({} stitches, {} transfers) for {} -> {}",
        doc.stitches.len(),
        compiled.vm_report.total_transfers,
        profile.id,
        out_dir.display()
    );
    Ok((result, 0, text))
}

fn cmd_knit_verify(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let doc_path = PathBuf::from(args.positional.get(2).ok_or_else(|| {
        bad_input(
            "usage: ftx knit verify PATTERN.knit.json --machine ID [--qual SWATCH.json] [--json]",
        )
    })?);
    let txt = std::fs::read_to_string(&doc_path).map_err(|e| workspace::io_error(&doc_path, e))?;
    let doc: KnitDoc = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("kniterate-7e");
    let profile = parse_machine(machine_id)?;
    let qual: Option<KnitSwatchQualification> = match args.options.get("qual") {
        Some(path) => {
            let p = PathBuf::from(path);
            let qtxt = std::fs::read_to_string(&p).map_err(|e| workspace::io_error(&p, e))?;
            Some(
                serde_json::from_str(&qtxt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };
    let compiled = compile_knit_doc(&doc, &profile, qual.as_ref(), None, None)?;
    let result = json!({
        "verified": true,
        "vm_report": compiled.vm_report,
        "cert": compiled.cert,
    });
    let text = format!(
        "Verified '{}' for {}: {} ops, {} transfers, max float {:.2} mm (hardware_execution_authorized=false)",
        doc.name,
        profile.id,
        compiled.vm_report.op_count,
        compiled.vm_report.total_transfers,
        compiled.vm_report.max_float_mm
    );
    Ok((result, 0, text))
}

fn cmd_knit_poincloux(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let mut params = PoinclouxParams::reference_monofilament();
    params.mu_friction = opt_f64(args, "mu", params.mu_friction)?;
    let strains = [0.0, 0.05, 0.10, 0.15, 0.20, 0.25, 0.30];
    let curve = poincloux_uniaxial_curve(&params, &strains)?;
    let rows: Vec<Value> = curve
        .iter()
        .map(|pt| {
            json!({
                "strain_wale": pt.strain_wale,
                "strain_course_free": pt.strain_course_free,
                "tension_frictionless_n_m": pt.tension_frictionless_n_m,
                "tension_loading_n_m": pt.tension_loading_n_m,
                "tension_unloading_n_m": pt.tension_unloading_n_m,
                "jamming_ratio": pt.jamming_ratio,
            })
        })
        .collect();
    let result = json!({
        "oracle": "poincloux_2018_planar_elastica",
        "mu_friction": params.mu_friction,
        "points": rows,
    });
    let text = format!(
        "Poincloux (2018) Rung-3 KAT (mu={:.2}): {} points, peak loading tension {:.2} N/m at strain 0.30",
        params.mu_friction,
        curve.len(),
        curve.last().map_or(0.0, |p| p.tension_loading_n_m)
    );
    Ok((result, 0, text))
}

/// Dispatches `ftx dye <optimize|verify> ...`.
pub(super) fn cmd_dye(args: &Args) -> Result<(Value, i32, String), Refusal> {
    match args.positional.get(1).map(String::as_str) {
        Some("optimize") => cmd_dye_optimize(args),
        Some("verify") => cmd_dye_verify(args),
        _ => Err(bad_input("expected `ftx dye optimize|verify`")),
    }
}

fn cmd_dye_optimize(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("piezo-cmyk");
    let machine = parse_dye_machine(machine_id)?;
    let age_str = args
        .options
        .get("age-class")
        .map(String::as_str)
        .unwrap_or("swatch");
    let wearer_age_class = parse_age_class(age_str)?;
    let regime = parse_regime(
        args.options
            .get("regime")
            .map(String::as_str)
            .unwrap_or("stripes"),
    )?;
    let courses = opt_u32(args, "courses", 12)?;
    let wales = opt_u32(args, "wales", 12)?;
    let is_circular = args.options.contains_key("circular");
    let drift = DriftParameters {
        systematic_eps: opt_f64(args, "drift-eps", 0.002)?,
        stochastic_sigma_eta: opt_f64(args, "drift-sigma", 0.015)?,
    };
    let wick_mm = opt_f64(args, "wick-mm", 1.5)?;
    let max_iters = opt_u32(args, "max-iters", 25)? as usize;

    let target = if let Some(img_path) = args.options.get("image") {
        let p = PathBuf::from(img_path);
        let bytes = std::fs::read(&p).map_err(|e| workspace::io_error(&p, e))?;
        load_target_raster_with_limit(&bytes, 16_000_000)?
    } else {
        // Built-in synthetic two-band stripe test target
        let mut px = Vec::with_capacity(64);
        for y in 0..8 {
            for _x in 0..8 {
                px.push(if y < 4 {
                    [35, 130, 185]
                } else {
                    [215, 165, 45]
                });
            }
        }
        TargetRaster::from_opaque_srgb(8, 8, px)?
    };

    let stitch_map = KnitStitchMap::regular_grid(0, courses, wales, 0.010, is_circular, 0.05);
    let mut optics = DyeOpticsModel::synthetic_cmyk_cotton();
    optics.primaries.truncate(machine.primaries.len());
    let n_p = optics.primaries.len();
    let n_cells = opt_u32(args, "cells", (courses * wales / 2).clamp(8, 128))? as usize;
    let tr_cfg = CapillaryTransportConfig {
        ds_m: stitch_map.total_yarn_length_m / (n_cells as f64),
        sigma_wick_m: wick_mm * 0.001,
        rf_retardations: machine
            .primaries
            .iter()
            .take(n_p)
            .map(|p| p.rf_retardation)
            .collect(),
        fixation_efficiency: vec![0.94; n_p],
        shrinkage_axial_frac: 0.02,
    };
    let inv_cfg = DyeInverseConfig {
        name: args
            .options
            .get("name")
            .cloned()
            .unwrap_or_else(|| "programmed_yarn_dye".to_string()),
        yarn_id: args
            .options
            .get("yarn")
            .cloned()
            .unwrap_or_else(|| "cotton-120tex".to_string()),
        wearer_age_class,
        regime,
        n_cells,
        n_sobol_scenarios: 8,
        cvar_alpha: 0.90,
        cvar_tau: 0.5,
        lambda_smooth: 0.05,
        lambda_switch: 0.02,
        max_iters,
        skin_underlayer_xyz: srgb_u8_to_xyz_d65([215, 180, 155]),
        ciede_eps_c: 1e-4,
        drift,
    };
    let opt =
        optimize_yarn_dye_program(&target, &stitch_map, &machine, &optics, &tr_cfg, &inv_cfg)?;
    let pareto = rank_five_regime_pareto(&target, &stitch_map, &machine, drift, 4);

    let qual: Option<YarnDyeingQualification> = match args.options.get("dye-qual") {
        Some(path) => {
            let p = PathBuf::from(path);
            let txt = std::fs::read_to_string(&p).map_err(|e| workspace::io_error(&p, e))?;
            Some(
                serde_json::from_str(&txt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };

    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx dye optimize` requires --out NEW_DIR"))?,
    );
    reserve_output(&out_dir, "dye optimize")?;

    let prog_json = serde_json::to_string_pretty(&opt.program)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let pareto_json = serde_json::to_string_pretty(&pareto)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    write_file(&out_dir, "dye_program.json", prog_json.as_bytes())?;
    write_file(&out_dir, "pareto_regimes.json", pareto_json.as_bytes())?;

    let mut emitted_controller = false;
    if wearer_age_class == WearerAgeClass::CalibrationSwatchOnly || qual.is_some() {
        let bundle = verify_and_export_dye_program(&opt.program, &machine, qual.as_ref(), None)?;
        write_file(&out_dir, "dye_schedule.csv", bundle.schedule_csv.as_bytes())?;
        write_file(
            &out_dir,
            "dye_controller.gcode",
            bundle.controller_gcode.as_bytes(),
        )?;
        write_file(&out_dir, "dye_cert.json", bundle.cert_json.as_bytes())?;
        emitted_controller = true;
    }

    let result = json!({
        "machine": machine.id,
        "regime": regime,
        "delta_e00_p50": opt.delta_e00_p50,
        "delta_e00_p95": opt.delta_e00_p95,
        "gamut_floor_delta_e00": opt.gamut_floor_delta_e00,
        "unreproducible_energy_fraction": opt.unreproducible_energy_fraction,
        "coherent_horizon_circular": opt.coherent_horizons.0,
        "coherent_horizon_flatbed": opt.coherent_horizons.1,
        "emitted_controller_files": emitted_controller,
        "pareto_regimes": pareto,
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
    });
    let text = format!(
        "Optimized yarn dye program ({} cells, {} iters): p50 dE00={:.2}, p95 dE00={:.2}, horizon circ/flat={}/{} st -> {}",
        n_cells,
        opt.iterations,
        opt.delta_e00_p50,
        opt.delta_e00_p95,
        opt.coherent_horizons.0,
        opt.coherent_horizons.1,
        out_dir.display()
    );
    Ok((result, 0, text))
}

fn cmd_dye_verify(args: &Args) -> Result<(Value, i32, String), Refusal> {
    let prog_path = PathBuf::from(
        args.positional
            .get(2)
            .ok_or_else(|| bad_input("usage: ftx dye verify PROGRAM.json --machine ID [--dye-qual Q.json] --out NEW_DIR [--json]"))?,
    );
    let txt =
        std::fs::read_to_string(&prog_path).map_err(|e| workspace::io_error(&prog_path, e))?;
    let prog: YarnDyeProgram = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let machine = parse_dye_machine(
        args.options
            .get("machine")
            .map(String::as_str)
            .unwrap_or("piezo-cmyk"),
    )?;
    let qual: Option<YarnDyeingQualification> = match args.options.get("dye-qual") {
        Some(path) => {
            let p = PathBuf::from(path);
            let qtxt = std::fs::read_to_string(&p).map_err(|e| workspace::io_error(&p, e))?;
            Some(
                serde_json::from_str(&qtxt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };
    let bundle = verify_and_export_dye_program(&prog, &machine, qual.as_ref(), None)?;
    if let Some(out) = args.options.get("out") {
        let out_dir = PathBuf::from(out);
        reserve_output(&out_dir, "dye verify")?;
        write_file(&out_dir, "dye_schedule.csv", bundle.schedule_csv.as_bytes())?;
        write_file(
            &out_dir,
            "dye_controller.gcode",
            bundle.controller_gcode.as_bytes(),
        )?;
        write_file(&out_dir, "dye_cert.json", bundle.cert_json.as_bytes())?;
    }
    let result = json!({
        "verified": true,
        "gcode_sha256": bundle.gcode_sha256,
        "cert": bundle.cert,
    });
    let text = format!(
        "Verified dye program '{}' for {}: peak wet tension {:.2} N, dwell [{:.2}, {:.2}] s (hardware_execution_authorized=false)",
        prog.name,
        machine.id,
        bundle.cert.hazard_report.peak_wet_tension_n,
        bundle.cert.hazard_report.min_oven_dwell_s,
        bundle.cert.hazard_report.max_oven_dwell_s
    );
    Ok((result, 0, text))
}

#[allow(dead_code)]
fn _carrier_unused_check() -> CarrierId {
    CarrierId("1".to_string())
}
