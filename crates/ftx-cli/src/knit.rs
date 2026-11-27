//! `ftx knit` and `ftx dye` subcommands (`FTX-K17`):
//! - `ftx knit swatch` — hard-capped `CalibrationSwatchOnly` kit (`.k`, AYAB, CYC SVG/PDF, cert, template)
//! - `ftx knit draft` — measured flat panel or shaped torso tube with child-safety gates
//! - `ftx knit verify` — 7-check V-bed VM & child-safety verifier for any `ftx-knit/1` document
//! - `ftx knit poincloux` — Rung-3 Poincloux et al. (2018) planar elastica analytical KAT curve
//! - `ftx dye optimize` — 4-stage radiometric KM + Sobol-CVaR Bertsekas L-BFGS + 5-regime Pareto
//! - `ftx dye verify` — physical dye-rig Capstan/dwell/Tg/accumulator/fastness verifier + `.csv`/`.gcode`/cert

use std::path::PathBuf;

use ftx_core::{Refusal, RefusalCode, TargetRaster};
use ftx_drape::dye::{
    CapillaryTransportConfig, DriftParameters, DyeInverseConfig, DyeOpticsModel,
    DyeUncertaintyContext, KnitStitchMap, optimize_yarn_dye_program_with_context,
    rank_five_regime_pareto, srgb_u8_to_xyz_d65,
};
use ftx_drape::knit::{PoinclouxParams, poincloux_uniaxial_curve};
use ftx_export::dye::{export_exp4_bleed_swatch_kit, verify_and_export_dye_program_with_receipt};
use ftx_export::knit::{
    check_hand_knit_pdf, check_hand_knit_svg, export_brother_ayab, export_calibration_swatch_kit,
    export_hand_knit_document, export_kniterate_kc, export_knitout_v2,
    export_tier_b_vendor_handoff, refuse_proprietary_binary_target,
};
use ftx_pattern::dye::{
    DyeMachineProfile, DyePlanUncertaintyReceipt, DyeingRegime, YarnDyeProgram,
    YarnDyeingQualification,
};
use ftx_pattern::knit::{
    GaugeSpec, KnitDoc, KnitSwatchQualification, MachineBackendKind, MachineProfile,
    ParametricPanelSpec, RaglanPulloverSpec, WearerAgeClass, compile_knit_doc,
    draft_seamless_raglan_tube, draft_shaped_panel,
};
use ftx_trace::raster::load_target_raster_with_limit;
use serde_json::{Value, json};

use super::{Args, bad_input, export, known_options, reserve_output, workspace};

const MAX_KNIT_FILE_BYTES: u64 = 64 * 1024 * 1024;
const SWATCH_MATERIAL_PLACEHOLDER: &str = "unmeasured-yarn-lot-placeholder";

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
        "stoll-12e-handoff" | "stoll-12e" | "stoll" => Ok(MachineProfile::stoll_12e_handoff()),
        "hand-knit" | "hand" => Ok(MachineProfile::hand_knit(4.0)),
        _ => Err(bad_input(format!(
            "unknown --machine '{id}' (expected kniterate-7e, brother-kh930, brother-kh270, shima-12e, stoll-12e, or hand-knit)"
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
        "child" | "child3_to14" | "child_3_to_14" => Ok(WearerAgeClass::Child3To14),
        "adult" => Ok(WearerAgeClass::Adult),
        "swatch" | "calibration_swatch_only" => Ok(WearerAgeClass::CalibrationSwatchOnly),
        _ => Err(bad_input(format!(
            "invalid --age-class '{raw}' (expected infant, child, adult, or swatch)"
        ))),
    }
}

fn parse_regime(raw: &str) -> Result<DyeingRegime, Refusal> {
    match raw {
        "stripes" | "1a" | "regime1_a_stripes" => Ok(DyeingRegime::Regime1AStripes),
        "pooling" | "1b" | "regime1_b_circular_pooling" => {
            Ok(DyeingRegime::Regime1BCircularPooling)
        }
        "fiducial" | "2" | "regime2_fiducial_synced" => Ok(DyeingRegime::Regime2FiducialSynced),
        "inline" | "3" | "regime3_inline_dancer" => Ok(DyeingRegime::Regime3InlineDancer),
        "jacquard" | "4" | "regime4_multi_carrier_jacquard" => {
            Ok(DyeingRegime::Regime4MultiCarrierJacquard)
        }
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
    workspace::write_new(&dir.join(name), bytes)
}

fn read_utf8(path: &std::path::Path, label: &str) -> Result<String, Refusal> {
    let bytes = crate::read_regular_bounded(path, MAX_KNIT_FILE_BYTES, label)?;
    String::from_utf8(bytes).map_err(|e| bad_input(format!("{}: {e}", path.display())))
}

fn read_knit_qualification(args: &Args) -> Result<Option<KnitSwatchQualification>, Refusal> {
    args.options
        .get("qual")
        .map(|path| {
            let txt = read_utf8(&PathBuf::from(path), "knit swatch qualification")?;
            serde_json::from_str(&txt)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
        })
        .transpose()
}

fn expected_knit_material(args: &Args) -> Result<Option<&str>, Refusal> {
    match args.options.get("material-sha256") {
        Some(value) if value.trim().is_empty() => Err(bad_input(
            "--material-sha256 must identify the selected source material",
        )),
        Some(value) => Ok(Some(value.as_str())),
        None => Ok(None),
    }
}

fn no_extra_positional(args: &Args, expected: usize, usage: &str) -> Result<(), Refusal> {
    if args.positional.len() != expected {
        return Err(bad_input(format!("usage: {usage}")));
    }
    Ok(())
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
    known_options(
        args,
        "knit swatch",
        &[
            "machine",
            "yarn",
            "needles",
            "courses-per-zone",
            "cams",
            "material-sha256",
            "created",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx knit swatch --out NEW_DIR [--machine ID] [--yarn ID] [--needles N] [--courses-per-zone N] [--cams A,B] [--created YYYY-MM-DDTHH:MM]",
    )?;
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
        Some(s) => {
            let parsed = s
                .split(',')
                .map(|tok| {
                    let v = tok
                        .trim()
                        .parse::<f64>()
                        .map_err(|_| bad_input(format!("invalid cam setting '{tok}'")))?;
                    Refusal::finite("cam setting", v)?;
                    Ok(v)
                })
                .collect::<Result<Vec<_>, Refusal>>()?;
            if parsed.is_empty() {
                return Err(bad_input("--cams must list at least one cam setting"));
            }
            parsed
        }
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

    let material_sha256 = expected_knit_material(args)?.unwrap_or(SWATCH_MATERIAL_PLACEHOLDER);
    let kit = export_calibration_swatch_kit(
        &profile,
        yarn_id,
        material_sha256,
        needles,
        courses_per_zone,
        &cams,
    )?;
    let compiled = compile_knit_doc(&kit.doc, &profile, None, Some(material_sha256), None)?;
    if compiled.cert.cert_sha256 != kit.knitout.cert.cert_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "calibration swatch kit certificate digest disagreed with compiled bundle",
        ));
    }
    let hand = export_hand_knit_document(&kit.doc, &created)?;
    let svg_checks = check_hand_knit_svg(&hand.svg_text, "swatch_chart.svg");
    let pdf_checks = check_hand_knit_pdf(&hand.pdf_bytes, "swatch_chart.pdf");
    if !svg_checks.iter().all(|c| c.ok) || !pdf_checks.iter().all(|c| c.ok) {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "hand-knit chart acceptance check failed",
        ));
    }
    // Prepare all backend outputs and serialized JSON before reserving --out so failures leave no partial directory.
    let doc_json = serde_json::to_string_pretty(&kit.doc)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let mut backend_export_refusal = None;
    let kc = if profile.backend_kind == MachineBackendKind::KniterateKc {
        match export_kniterate_kc(&compiled, &profile) {
            Ok(bundle) => Some(bundle),
            Err(refusal) if refusal.code == RefusalCode::UnqualifiedProcess => {
                backend_export_refusal = Some(refusal);
                None
            }
            Err(refusal) => return Err(refusal),
        }
    } else {
        None
    };
    let ayab = if profile.backend_kind == MachineBackendKind::BrotherAyab {
        Some(export_brother_ayab(&kit.doc, &profile, &compiled.cert)?)
    } else {
        None
    };
    let handoff = if profile.backend_kind.is_opaque_proprietary() {
        let fmt = match profile.backend_kind {
            MachineBackendKind::ShimaSeikiHandoff => ".dat",
            MachineBackendKind::StollHandoff => "SINTRAL",
            _ => "proprietary",
        };
        Some(export_tier_b_vendor_handoff(&compiled, &profile, fmt)?)
    } else {
        None
    };

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

    if let Some(kc) = kc {
        write_file(&out_dir, "swatch.kc", kc.kc_text.as_bytes())?;
        files.push("swatch.kc".to_string());
    } else if let Some(ayab) = ayab {
        write_file(&out_dir, "swatch_ayab.png", &ayab.png_bytes)?;
        write_file(
            &out_dir,
            "ayab_session_checklist.json",
            ayab.checklist_json.as_bytes(),
        )?;
        files.push("swatch_ayab.png".to_string());
        files.push("ayab_session_checklist.json".to_string());
    } else if let Some(handoff) = handoff {
        write_file(
            &out_dir,
            "vendor_handoff_manifest.json",
            handoff.manifest_json.as_bytes(),
        )?;
        files.push("vendor_handoff_manifest.json".to_string());
    }

    let conversion_notice = backend_export_refusal
        .as_ref()
        .map_or_else(String::new, |refusal| {
            format!("\nMachine conversion required: {}", refusal.message)
        });
    let result = json!({
        "machine": profile.id,
        "wearer_age_class": "calibration_swatch_only",
        "out_dir": out_dir.display().to_string(),
        "files": files,
        "knitout_sha256": kit.knitout.knitout_sha256,
        "cert_sha256": kit.knitout.cert.cert_sha256,
        "backend_export_refusal": backend_export_refusal,
        "machine_executable": false,
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
    });
    let text = format!(
        "Generated CalibrationSwatchOnly kit for {} in {} ({} files; hardware_execution_authorized=false){conversion_notice}",
        profile.id,
        out_dir.display(),
        files.len()
    );
    Ok((result, 0, text))
}

fn cmd_knit_draft(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit draft",
        &[
            "style",
            "machine",
            "allow-proprietary-binary",
            "age-class",
            "wales",
            "courses",
            "yarn-tex",
            "thickness-mm",
            "yarn",
            "cam",
            "has-drawstring",
            "name",
            "width-mm",
            "chest-mm",
            "length-mm",
            "armhole-mm",
            "armhole-inset-mm",
            "neck-mm",
            "neck-depth-mm",
            "rib-courses",
            "yoke-mm",
            "collar-rib",
            "qual",
            "material-sha256",
            "created",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx knit draft --age-class infant|child|adult --out NEW_DIR [--style panel|tube] [--machine ID] [--qual SWATCH.json --material-sha256 SOURCE_DIGEST]",
    )?;
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
    let qual = read_knit_qualification(args)?;
    let expected_material_sha256 = expected_knit_material(args)?;
    let wales_per_100mm = opt_f64(
        args,
        "wales",
        qual.as_ref().map_or(24.0, |q| q.wales_per_100mm),
    )?;
    let courses_per_100mm = opt_f64(
        args,
        "courses",
        qual.as_ref().map_or(32.0, |q| q.courses_per_100mm),
    )?;
    let yarn_tex = args
        .options
        .get("yarn-tex")
        .map(|s| {
            let v = s
                .parse::<f64>()
                .map_err(|_| bad_input(format!("--yarn-tex must be a number, got '{s}'")))?;
            Refusal::finite("yarn-tex", v)?;
            if v <= 0.0 {
                return Err(Refusal::new(
                    RefusalCode::OutOfRange,
                    "--yarn-tex must be positive",
                ));
            }
            Ok(v)
        })
        .transpose()?;
    let gauge = GaugeSpec {
        wales_per_100mm,
        courses_per_100mm,
        needle_pitch_mm: profile.needle_pitch_mm,
        yarn_tex,
        thickness_mm: Some(opt_f64(
            args,
            "thickness-mm",
            qual.as_ref().map_or(2.0, |q| q.thickness_mm),
        )?),
        relaxed_after_wash: qual.as_ref().is_some_and(|q| q.was_washed_and_blocked),
    };
    let yarn_id = args.options.get("yarn").cloned().unwrap_or_else(|| {
        qual.as_ref()
            .map_or_else(|| "unspecified-yarn".to_string(), |q| q.yarn_lot_id.clone())
    });
    let cam = opt_f64(
        args,
        "cam",
        qual.as_ref().map_or(
            0.5 * (profile.cam_setting_range[0] + profile.cam_setting_range[1]),
            |q| q.cam_stitch_setting,
        ),
    )?;
    let has_drawstring = args.options.contains_key("has-drawstring");

    let mut doc = match style {
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
                neck_width_mm: opt_f64(args, "neck-mm", 0.0)?,
                neck_depth_mm: opt_f64(args, "neck-depth-mm", 0.0)?,
                rib_hem_courses: opt_u32(
                    args,
                    "rib-courses",
                    if profile.has_back_bed { 8 } else { 0 },
                )?,
                cam_setting: cam,
                yarn_id,
                carrier_id: profile.carrier_ids[0].clone(),
                has_hood_or_neck_drawstring: has_drawstring,
            },
            &gauge,
            &profile,
        )?,
        "tube" | "raglan" => draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: args
                    .options
                    .get("name")
                    .cloned()
                    .unwrap_or_else(|| "shaped_torso_tube".to_string()),
                wearer_age_class,
                neck_girth_mm: opt_f64(args, "neck-mm", 280.0)?,
                chest_girth_mm: opt_f64(args, "chest-mm", 560.0)?,
                yoke_depth_mm: opt_f64(args, "yoke-mm", 120.0)?,
                body_length_below_arm_mm: opt_f64(args, "length-mm", 200.0)?,
                collar_rib_courses: opt_u32(args, "collar-rib", 0)?,
                hem_rib_courses: opt_u32(args, "rib-courses", 0)?,
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
                "unknown --style '{style}' (expected panel or tube; raglan is a legacy tube alias)"
            )));
        }
    };

    if let Some(material_sha256) = expected_material_sha256 {
        doc.metadata
            .insert("material_sha256".to_string(), material_sha256.to_string());
    }
    if let Some(q) = &qual {
        doc.metadata
            .insert("yarn_lot_id".to_string(), q.yarn_lot_id.clone());
    }
    let compiled = compile_knit_doc(
        &doc,
        &profile,
        qual.as_ref(),
        expected_material_sha256,
        None,
    )?;
    let knitout = export_knitout_v2(&compiled)?;
    let created = match args.options.get("created") {
        Some(c) => c.clone(),
        None => export::utc_minute()?,
    };
    let hand = export_hand_knit_document(&doc, &created)?;
    let svg_checks = check_hand_knit_svg(&hand.svg_text, "chart.svg");
    let pdf_checks = check_hand_knit_pdf(&hand.pdf_bytes, "chart.pdf");
    if !svg_checks.iter().all(|c| c.ok) || !pdf_checks.iter().all(|c| c.ok) {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "hand-knit chart acceptance check failed",
        ));
    }

    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit draft` requires --out NEW_DIR"))?,
    );
    let doc_json = serde_json::to_string_pretty(&doc)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let mut backend_export_refusal = None;
    let kc = if profile.backend_kind == MachineBackendKind::KniterateKc {
        match export_kniterate_kc(&compiled, &profile) {
            Ok(bundle) => Some(bundle),
            Err(refusal) if refusal.code == RefusalCode::UnqualifiedProcess => {
                backend_export_refusal = Some(refusal);
                None
            }
            Err(refusal) => return Err(refusal),
        }
    } else {
        None
    };
    let ayab = if profile.backend_kind == MachineBackendKind::BrotherAyab {
        Some(export_brother_ayab(&doc, &profile, &compiled.cert)?)
    } else {
        None
    };
    let handoff = if profile.backend_kind.is_opaque_proprietary() {
        let fmt = match profile.backend_kind {
            MachineBackendKind::ShimaSeikiHandoff => ".dat",
            MachineBackendKind::StollHandoff => "SINTRAL",
            _ => "proprietary",
        };
        Some(export_tier_b_vendor_handoff(&compiled, &profile, fmt)?)
    } else {
        None
    };

    reserve_output(&out_dir, "knit draft")?;

    let mut files = vec![
        "pattern.knit.json".to_string(),
        "program.k".to_string(),
        "cert.json".to_string(),
        "chart.svg".to_string(),
        "chart.pdf".to_string(),
        "instructions.txt".to_string(),
    ];
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

    if let Some(kc) = kc {
        write_file(&out_dir, "program.kc", kc.kc_text.as_bytes())?;
        files.push("program.kc".to_string());
    } else if let Some(ayab) = ayab {
        write_file(&out_dir, "pattern_ayab.png", &ayab.png_bytes)?;
        write_file(
            &out_dir,
            "ayab_session_checklist.json",
            ayab.checklist_json.as_bytes(),
        )?;
        files.push("pattern_ayab.png".to_string());
        files.push("ayab_session_checklist.json".to_string());
    } else if let Some(handoff) = handoff {
        write_file(
            &out_dir,
            "vendor_handoff_manifest.json",
            handoff.manifest_json.as_bytes(),
        )?;
        files.push("vendor_handoff_manifest.json".to_string());
    }

    let conversion_notice = backend_export_refusal
        .as_ref()
        .map_or_else(String::new, |refusal| {
            format!("\nMachine conversion required: {}", refusal.message)
        });
    let result = json!({
        "style": style,
        "machine": profile.id,
        "wearer_age_class": doc.wearer_age_class,
        "stitches": doc.stitches.len(),
        "transfers": compiled.vm_report.total_transfers,
        "max_float_mm": compiled.vm_report.max_float_mm,
        "files": files,
        "metadata": doc.metadata,
        "backend_export_refusal": backend_export_refusal,
        "machine_executable": false,
        "cert": compiled.cert,
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    let text = format!(
        "Drafted & verified {style} knit ({} stitches, {} transfers) for {} -> {}{conversion_notice}",
        doc.stitches.len(),
        compiled.vm_report.total_transfers,
        profile.id,
        out_dir.display()
    );
    Ok((result, 0, text))
}

fn cmd_knit_verify(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "knit verify", &["machine", "qual", "material-sha256"])?;
    no_extra_positional(
        args,
        3,
        "ftx knit verify PATTERN.knit.json [--machine ID] [--qual SWATCH.json --material-sha256 SOURCE_DIGEST] [--json]",
    )?;
    let doc_path = PathBuf::from(&args.positional[2]);
    let txt = read_utf8(&doc_path, "knit pattern document")?;
    let doc: KnitDoc = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("kniterate-7e");
    let profile = parse_machine(machine_id)?;
    let qual = read_knit_qualification(args)?;
    let compiled = compile_knit_doc(
        &doc,
        &profile,
        qual.as_ref(),
        expected_knit_material(args)?,
        None,
    )?;
    let _ = export_knitout_v2(&compiled)?;
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
    known_options(args, "knit poincloux", &["mu"])?;
    no_extra_positional(args, 2, "ftx knit poincloux [--mu MU] [--json]")?;
    let mut params = PoinclouxParams::reference_monofilament();
    params.mu_friction = opt_f64(args, "mu", params.mu_friction)?;
    Refusal::finite("mu", params.mu_friction)?;
    if params.mu_friction < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "--mu cannot be negative",
        ));
    }
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

/// Dispatches `ftx dye <swatch|optimize|verify> ...`.
pub(super) fn cmd_dye(args: &Args) -> Result<(Value, i32, String), Refusal> {
    match args.positional.get(1).map(String::as_str) {
        Some("swatch") => cmd_dye_swatch(args),
        Some("optimize") => cmd_dye_optimize(args),
        Some("verify") => cmd_dye_verify(args),
        _ => Err(bad_input("expected `ftx dye swatch|optimize|verify`")),
    }
}

fn cmd_dye_swatch(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(args, "dye swatch", &["machine", "yarn", "out"])?;
    no_extra_positional(
        args,
        2,
        "ftx dye swatch --out NEW_DIR [--machine piezo-cmyk|dip-3bath] [--yarn ID] [--json]",
    )?;
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("piezo-cmyk");
    let machine = parse_dye_machine(machine_id)?;
    let yarn_id = args
        .options
        .get("yarn")
        .map(String::as_str)
        .unwrap_or("cotton-120tex");
    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx dye swatch` requires --out NEW_DIR"))?,
    );
    let kit = export_exp4_bleed_swatch_kit(&machine, yarn_id, SWATCH_MATERIAL_PLACEHOLDER)?;
    let prog_json = serde_json::to_string_pretty(&kit.program)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;

    reserve_output(&out_dir, "dye swatch")?;
    let files = vec![
        "exp4_dye_program.json".to_string(),
        "exp4_schedule.csv".to_string(),
        "exp4_controller.gcode".to_string(),
        "exp4_cert.json".to_string(),
        "qualification_template.json".to_string(),
    ];
    write_file(&out_dir, "exp4_dye_program.json", prog_json.as_bytes())?;
    write_file(
        &out_dir,
        "exp4_schedule.csv",
        kit.export.schedule_csv.as_bytes(),
    )?;
    write_file(
        &out_dir,
        "exp4_controller.gcode",
        kit.export.controller_gcode.as_bytes(),
    )?;
    write_file(&out_dir, "exp4_cert.json", kit.export.cert_json.as_bytes())?;
    write_file(
        &out_dir,
        "qualification_template.json",
        kit.qualification_template_json.as_bytes(),
    )?;

    let result = json!({
        "machine": machine.id,
        "wearer_age_class": "calibration_swatch_only",
        "not_for_garment_wear": true,
        "out_dir": out_dir.display().to_string(),
        "files": files,
        "gcode_sha256": kit.export.gcode_sha256,
        "cert_sha256": kit.export.cert.cert_sha256,
        "hardware_execution_authorized": false,
        "physical_dyeing_validated": false,
    });
    let text = format!(
        "Generated Exp-4 dye bleed/fastness swatch kit for {} in {} ({} files; not_for_garment_wear=true)",
        machine.id,
        out_dir.display(),
        files.len()
    );
    Ok((result, 0, text))
}

fn cmd_dye_optimize(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "dye optimize",
        &[
            "machine",
            "age-class",
            "regime",
            "courses",
            "wales",
            "circular",
            "drift-eps",
            "drift-sigma",
            "wick-mm",
            "max-iters",
            "image",
            "cells",
            "name",
            "yarn",
            "dye-qual",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx dye optimize --out NEW_DIR [--machine ID] [--age-class CLASS] [--regime R] [--courses N] [--wales N] [--image PATH] [--dye-qual Q.json]",
    )?;
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
    if !(2..=256).contains(&courses) || !(2..=256).contains(&wales) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!("dye grid {courses}x{wales} must be within 2..=256 courses and 2..=256 wales"),
        ));
    }
    let is_circular = args.options.contains_key("circular");
    let drift = DriftParameters {
        systematic_eps: opt_f64(args, "drift-eps", 0.002)?,
        stochastic_sigma_eta: opt_f64(args, "drift-sigma", 0.015)?,
    };
    let qual: Option<YarnDyeingQualification> = match args.options.get("dye-qual") {
        Some(path) => {
            let p = PathBuf::from(path);
            let txt = read_utf8(&p, "yarn dyeing qualification")?;
            Some(
                serde_json::from_str(&txt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };
    let wick_mm = match args.options.get("wick-mm") {
        Some(_) => opt_f64(args, "wick-mm", 1.5)?,
        None => qual.as_ref().map_or(1.5, |q| q.wick_dispersion_mm),
    };
    Refusal::finite("drift-eps", drift.systematic_eps)?;
    Refusal::finite("drift-sigma", drift.stochastic_sigma_eta)?;
    Refusal::finite("wick-mm", wick_mm)?;
    if drift.stochastic_sigma_eta < 0.0 || wick_mm < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "--drift-sigma and --wick-mm cannot be negative",
        ));
    }
    let max_iters = opt_u32(args, "max-iters", 25)? as usize;
    if !(1..=200).contains(&max_iters) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("--max-iters {max_iters} must be within 1..=200"),
        ));
    }

    let target = if let Some(img_path) = args.options.get("image") {
        let p = PathBuf::from(img_path);
        let bytes = crate::read_regular_bounded(&p, MAX_KNIT_FILE_BYTES, "dye target image")?;
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
    let default_cells = courses
        .saturating_mul(wales)
        .saturating_div(2)
        .clamp(8, 128);
    let n_cells = opt_u32(args, "cells", default_cells)? as usize;
    if !(1..=2048).contains(&n_cells) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!("--cells {n_cells} must be within 1..=2048"),
        ));
    }
    let shrinkage_axial_frac = qual.as_ref().map_or(0.02, |q| q.shrinkage_axial_pct * 0.01);
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
        shrinkage_axial_frac,
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
    // `cmd_dye_optimize` currently solves against `DyeOpticsModel::synthetic_cmyk_cotton()` rather than
    // measured spectral K/S cards from the qualification record, so keep optical_measurement_basis
    // UnqualifiedUncalibrated and evidence Estimate even when binding `qualification_sha256`.
    let uncertainty_ctx = DyeUncertaintyContext {
        qualification_sha256: qual
            .as_ref()
            .map(YarnDyeingQualification::canonical_sha256)
            .transpose()?,
        ..DyeUncertaintyContext::default()
    };
    let opt = optimize_yarn_dye_program_with_context(
        &target,
        &stitch_map,
        &machine,
        &optics,
        &tr_cfg,
        &inv_cfg,
        &uncertainty_ctx,
    )?;
    let pareto = rank_five_regime_pareto(&target, &stitch_map, &machine, drift, 4);

    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx dye optimize` requires --out NEW_DIR"))?,
    );
    let prog_json = serde_json::to_string_pretty(&opt.program)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let receipt_json = serde_json::to_string_pretty(&opt.uncertainty_receipt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let pareto_json = serde_json::to_string_pretty(&pareto)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let controller_bundle =
        if wearer_age_class == WearerAgeClass::CalibrationSwatchOnly || qual.is_some() {
            Some(verify_and_export_dye_program_with_receipt(
                &opt.program,
                &machine,
                qual.as_ref(),
                None,
                Some(&opt.uncertainty_receipt),
            )?)
        } else {
            None
        };

    reserve_output(&out_dir, "dye optimize")?;
    write_file(&out_dir, "dye_program.json", prog_json.as_bytes())?;
    write_file(
        &out_dir,
        "dye_uncertainty_receipt.json",
        receipt_json.as_bytes(),
    )?;
    write_file(&out_dir, "pareto_regimes.json", pareto_json.as_bytes())?;

    let emitted_controller = if let Some(bundle) = controller_bundle {
        write_file(&out_dir, "dye_schedule.csv", bundle.schedule_csv.as_bytes())?;
        write_file(
            &out_dir,
            "dye_controller.gcode",
            bundle.controller_gcode.as_bytes(),
        )?;
        write_file(&out_dir, "dye_cert.json", bundle.cert_json.as_bytes())?;
        true
    } else {
        false
    };

    let result = json!({
        "machine": machine.id,
        "regime": regime,
        "delta_e00_p50": opt.delta_e00_p50,
        "delta_e00_p95": opt.delta_e00_p95,
        "gamut_floor_delta_e00": opt.gamut_floor_delta_e00,
        "unreproducible_energy_fraction": opt.unreproducible_energy_fraction,
        "coherent_horizon_circular": opt.coherent_horizons.0,
        "coherent_horizon_flatbed": opt.coherent_horizons.1,
        "uncertainty_receipt_sha256": opt.uncertainty_receipt.payload_sha256,
        "uncertainty_receipt": opt.uncertainty_receipt,
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
    known_options(
        args,
        "dye verify",
        &["machine", "dye-qual", "receipt", "out"],
    )?;
    no_extra_positional(
        args,
        3,
        "ftx dye verify PROGRAM.json [--machine ID] [--dye-qual Q.json] [--receipt RECEIPT.json] [--out NEW_DIR] [--json]",
    )?;
    let prog_path = PathBuf::from(&args.positional[2]);
    let txt = read_utf8(&prog_path, "dye program document")?;
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
            let qtxt = read_utf8(&p, "yarn dyeing qualification")?;
            Some(
                serde_json::from_str(&qtxt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };
    let receipt: Option<DyePlanUncertaintyReceipt> = match args.options.get("receipt") {
        Some(path) => {
            let p = PathBuf::from(path);
            let rtxt = read_utf8(&p, "dye plan uncertainty receipt")?;
            Some(
                serde_json::from_str(&rtxt)
                    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            )
        }
        None => None,
    };
    let bundle = verify_and_export_dye_program_with_receipt(
        &prog,
        &machine,
        qual.as_ref(),
        None,
        receipt.as_ref(),
    )?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    fn fresh_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ftx-knit-cli-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn knit_and_dye_cli_validate_options_bounds_and_round_trip() {
        let root = fresh_dir("roundtrip");
        let swatch_dir = root.join("swatch");
        let swatch_args = parse(
            &[
                "knit",
                "swatch",
                "--machine",
                "kniterate-7e",
                "--needles",
                "20",
                "--courses-per-zone",
                "8",
                "--cams",
                "5.5,7.5",
                "--created",
                "2026-11-27T12:00",
                "--out",
                swatch_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (swatch_val, code, _) = cmd_knit(&swatch_args).unwrap();
        assert_eq!(code, 0);
        assert_eq!(swatch_val["hardware_execution_authorized"], false);
        assert!(swatch_dir.join("swatch.knit.json").is_file());
        assert!(swatch_dir.join("swatch.k").is_file());
        assert!(!swatch_dir.join("swatch.kc").exists());
        assert_eq!(swatch_val["machine_executable"], false);
        assert_eq!(
            swatch_val["backend_export_refusal"]["code"],
            serde_json::to_value(RefusalCode::UnqualifiedProcess).unwrap()
        );

        // Reusing --out must refuse without overwriting.
        assert!(cmd_knit(&swatch_args).is_err());

        // Verify the generated swatch document cleanly.
        let verify_args = parse(
            &[
                "knit",
                "verify",
                swatch_dir.join("swatch.knit.json").to_str().unwrap(),
                "--machine",
                "kniterate-7e",
            ]
            .map(String::from),
        )
        .unwrap();
        let (ver_val, ver_code, _) = cmd_knit(&verify_args).unwrap();
        assert_eq!(ver_code, 0);
        assert_eq!(ver_val["verified"], true);

        // Unknown options and extra positionals must refuse (never silently drop safety flags).
        // A misspelled boolean flag with no value fails closed during CLI parsing.
        assert!(
            parse(
                &[
                    "knit",
                    "draft",
                    "--age-class",
                    "infant",
                    "--has-drawstrings",
                    "--out",
                    root.join("never-created-1").to_str().unwrap(),
                ]
                .map(String::from),
            )
            .is_err()
        );
        // An unknown valued option reaches `known_options` and must refuse before touching disk.
        let unknown_opt = parse(
            &[
                "knit",
                "draft",
                "--age-class",
                "infant",
                "--qual-file",
                "q.json",
                "--out",
                root.join("never-created-1").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_knit(&unknown_opt).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        assert!(!root.join("never-created-1").exists());

        // Extra positional arguments must also refuse.
        let extra_pos = parse(
            &[
                "knit",
                "verify",
                swatch_dir.join("swatch.knit.json").to_str().unwrap(),
                "unexpected-extra.json",
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_knit(&extra_pos).unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        // Invalid --yarn-tex must refuse instead of silently turning into None.
        let bad_tex = parse(
            &[
                "knit",
                "draft",
                "--age-class",
                "adult",
                "--yarn-tex",
                "not-a-number",
                "--out",
                root.join("never-created-2").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_knit(&bad_tex).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        assert!(!root.join("never-created-2").exists());

        // Dye bounds refuse overflow/zero-cells/negative wick before touching disk.
        let bad_grid = parse(
            &[
                "dye",
                "optimize",
                "--courses",
                "100000",
                "--wales",
                "100000",
                "--out",
                root.join("never-created-3").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_dye(&bad_grid).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        let zero_cells = parse(
            &[
                "dye",
                "optimize",
                "--courses",
                "6",
                "--wales",
                "6",
                "--cells",
                "0",
                "--out",
                root.join("never-created-4").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_dye(&zero_cells).unwrap_err().code,
            RefusalCode::OutOfRange
        );

        // Valid dye optimize -> verify round-trip.
        let dye_dir = root.join("dye");
        let dye_args = parse(
            &[
                "dye",
                "optimize",
                "--courses",
                "6",
                "--wales",
                "6",
                "--max-iters",
                "6",
                "--out",
                dye_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (dye_val, dye_code, _) = cmd_dye(&dye_args).unwrap();
        assert_eq!(dye_code, 0);
        assert_eq!(dye_val["emitted_controller_files"], true);
        assert!(dye_dir.join("dye_uncertainty_receipt.json").is_file());
        assert_eq!(
            dye_val["uncertainty_receipt"]["four_claims"]["color_fidelity_claimed"],
            false
        );
        let dye_verify_args = parse(
            &[
                "dye",
                "verify",
                dye_dir.join("dye_program.json").to_str().unwrap(),
                "--machine",
                "piezo-cmyk",
                "--receipt",
                dye_dir
                    .join("dye_uncertainty_receipt.json")
                    .to_str()
                    .unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (dv_val, dv_code, _) = cmd_dye(&dye_verify_args).unwrap();
        assert_eq!(dv_code, 0);
        assert_eq!(dv_val["verified"], true);
        assert_eq!(
            dv_val["cert"]["uncertainty_receipt_sha256"].as_str(),
            dye_val["uncertainty_receipt_sha256"].as_str()
        );

        let mut tampered_receipt: Value = serde_json::from_str(
            &std::fs::read_to_string(dye_dir.join("dye_uncertainty_receipt.json")).unwrap(),
        )
        .unwrap();
        tampered_receipt["four_claims"]["color_fidelity_claimed"] = json!(true);
        let tampered_receipt_path = root.join("tampered_dye_receipt.json");
        std::fs::write(
            &tampered_receipt_path,
            serde_json::to_string_pretty(&tampered_receipt).unwrap(),
        )
        .unwrap();
        let bad_verify_args = parse(
            &[
                "dye",
                "verify",
                dye_dir.join("dye_program.json").to_str().unwrap(),
                "--machine",
                "piezo-cmyk",
                "--receipt",
                tampered_receipt_path.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_dye(&bad_verify_args).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );

        // Adult `dye optimize --dye-qual` binds qualification_sha256 and uses measured wick/shrinkage
        // while keeping `color_fidelity_claimed = false` because CLI still uses synthetic K/S optics.
        let piezo = DyeMachineProfile::benchtop_piezo_cmyk();
        let adult_qual = YarnDyeingQualification {
            id: "dq-cli-adult".to_string(),
            material_sha256: "cotton-120tex-sha".to_string(),
            dye_machine_sha256: piezo.canonical_sha256().unwrap(),
            yarn_lot_id: "lot-1".to_string(),
            dye_lot_id: "dye-1".to_string(),
            wick_dispersion_mm: 1.8,
            shrinkage_axial_pct: 4.5,
            wet_tenacity_retention_ratio: 0.92,
            yarn_breaking_force_n: 8.5,
            yarn_tg_c: None,
            optical_measurement_basis:
                ftx_pattern::dye::OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
            washing_off_validated: true,
            iso_105_c06_grade: Some(4.5),
            iso_105_x12_wet_rub_grade: Some(4.5),
            saliva_fastness_validated: true,
            evidence: ftx_pattern::materials::Evidence::Measured,
            source: ftx_pattern::materials::Source {
                reference: "local:cli_exp4_test".to_string(),
                accessed_on: "2026-11-27".to_string(),
                note: "synthetic test fixture".to_string(),
            },
            method: "steam fix + wash".to_string(),
            conditioning: std::collections::BTreeMap::new(),
            allow_estimates: false,
        };
        let adult_qual_path = root.join("adult_dye_qual.json");
        std::fs::write(
            &adult_qual_path,
            serde_json::to_string_pretty(&adult_qual).unwrap(),
        )
        .unwrap();
        let adult_dye_dir = root.join("dye-adult");
        let adult_dye_args = parse(
            &[
                "dye",
                "optimize",
                "--age-class",
                "adult",
                "--dye-qual",
                adult_qual_path.to_str().unwrap(),
                "--courses",
                "4",
                "--wales",
                "4",
                "--max-iters",
                "4",
                "--out",
                adult_dye_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (adult_dye_val, adult_dye_code, _) = cmd_dye(&adult_dye_args).unwrap();
        assert_eq!(adult_dye_code, 0);
        assert_eq!(adult_dye_val["emitted_controller_files"], true);
        assert_eq!(
            adult_dye_val["uncertainty_receipt"]["four_claims"]["color_fidelity_claimed"],
            false
        );
        assert_eq!(
            adult_dye_val["uncertainty_receipt"]["dressed_appearance"]["differential_shrinkage_stretch_perturbation_flagged"],
            true
        );
        assert_eq!(
            adult_dye_val["uncertainty_receipt"]["qualification_sha256"].as_str(),
            Some(adult_qual.canonical_sha256().unwrap().as_str())
        );

        // Brother KH-930 AYAB swatch emits 1-bit PNG + checklist and seals matching cert.json.
        let ayab_dir = root.join("ayab-swatch");
        let ayab_args = parse(
            &[
                "knit",
                "swatch",
                "--machine",
                "brother-kh930",
                "--needles",
                "16",
                "--courses-per-zone",
                "6",
                "--cams",
                "4.0,6.0",
                "--created",
                "2026-11-27T12:00",
                "--out",
                ayab_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (ayab_val, ayab_code, _) = cmd_knit(&ayab_args).unwrap();
        assert_eq!(ayab_code, 0);
        assert!(ayab_dir.join("swatch_ayab.png").is_file());
        assert!(ayab_dir.join("ayab_session_checklist.json").is_file());
        let cert_json: Value =
            serde_json::from_str(&std::fs::read_to_string(ayab_dir.join("cert.json")).unwrap())
                .unwrap();
        let qual_tpl_json: Value = serde_json::from_str(
            &std::fs::read_to_string(ayab_dir.join("qualification_template.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(
            qual_tpl_json["material_sha256"].as_str(),
            Some(SWATCH_MATERIAL_PLACEHOLDER)
        );
        assert_eq!(cert_json["automated_carriage_control"], false);
        assert_eq!(
            cert_json["cert_sha256"].as_str(),
            ayab_val["cert_sha256"].as_str()
        );

        // Tier-B Shima swatch emits verified Knitout v2 + vendor_handoff_manifest.json (machine_executable=false).
        let shima_swatch_dir = root.join("shima-swatch");
        let shima_swatch_args = parse(
            &[
                "knit",
                "swatch",
                "--machine",
                "shima-12e",
                "--needles",
                "16",
                "--courses-per-zone",
                "6",
                "--cams",
                "5.0,7.0",
                "--created",
                "2026-11-27T12:00",
                "--out",
                shima_swatch_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (_, shima_code, _) = cmd_knit(&shima_swatch_args).unwrap();
        assert_eq!(shima_code, 0);
        let manifest_json: Value = serde_json::from_str(
            &std::fs::read_to_string(shima_swatch_dir.join("vendor_handoff_manifest.json"))
                .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest_json["machine_executable"], false);
        assert_eq!(manifest_json["hardware_execution_authorized"], false);

        // Exp-4 dye swatch kit emits verified files with not_for_garment_wear=true.
        let dye_swatch_dir = root.join("dye-swatch");
        let dye_swatch_args = parse(
            &[
                "dye",
                "swatch",
                "--machine",
                "piezo-cmyk",
                "--yarn",
                "cotton-120tex",
                "--out",
                dye_swatch_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (ds_val, ds_code, _) = cmd_dye(&dye_swatch_args).unwrap();
        assert_eq!(ds_code, 0);
        assert_eq!(ds_val["not_for_garment_wear"], true);
        assert_eq!(ds_val["hardware_execution_authorized"], false);
        assert!(dye_swatch_dir.join("exp4_controller.gcode").is_file());
        assert!(dye_swatch_dir.join("qualification_template.json").is_file());

        // Hand-knit draft reports honesty flags & files; Shima proprietary binary refuses fail-closed.
        let hand_dir = root.join("hand-panel");
        let hand_args = parse(
            &[
                "knit",
                "draft",
                "--machine",
                "hand-knit",
                "--age-class",
                "infant",
                "--created",
                "2026-11-27T12:00",
                "--out",
                hand_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let (hand_val, hand_code, _) = cmd_knit(&hand_args).unwrap();
        assert_eq!(hand_code, 0);
        assert_eq!(hand_val["hardware_execution_authorized"], false);
        assert_eq!(hand_val["physical_knitting_validated"], false);
        assert_eq!(hand_val["fit_claimed"], false);
        assert!(hand_dir.join("chart.svg").is_file());
        assert!(hand_dir.join("chart.pdf").is_file());

        let shima_prop = parse(
            &[
                "knit",
                "draft",
                "--machine",
                "shima-12e",
                "--age-class",
                "adult",
                "--allow-proprietary-binary",
                "--out",
                root.join("never-created-shima").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_knit(&shima_prop).unwrap_err().code,
            RefusalCode::OpaqueProprietaryTarget
        );
        assert!(!root.join("never-created-shima").exists());
    }

    #[test]
    fn production_draft_uses_qualified_gauge_and_requires_independent_material_identity() {
        let root = fresh_dir("qualified-panel");
        let profile = MachineProfile::kniterate_7e();
        let material_sha256 = "a".repeat(64);
        let qualification = json!({
            "id": "synthetic-washed-swatch",
            "material_sha256": material_sha256,
            "machine_sha256": profile.canonical_sha256().unwrap(),
            "yarn_lot_id": "synthetic-lot",
            "was_washed_and_blocked": true,
            "wales_per_100mm": 20.0,
            "courses_per_100mm": 25.0,
            "thickness_mm": 1.6,
            "cam_stitch_setting": 6.0,
            "evidence": ftx_pattern::materials::Evidence::Measured,
            "source": {
                "reference": "synthetic:test",
                "accessed_on": "2026-11-27",
                "note": "synthetic compiler regression, not measured fabric"
            },
            "method": "synthetic exact gauge",
            "conditioning": {}
        });
        let qual_path = root.join("qualification.json");
        std::fs::write(&qual_path, serde_json::to_vec(&qualification).unwrap()).unwrap();
        let out_dir = root.join("panel");
        let mut args = parse(
            &[
                "knit",
                "draft",
                "--age-class",
                "adult",
                "--machine",
                "kniterate-7e",
                "--qual",
                qual_path.to_str().unwrap(),
                "--material-sha256",
                &material_sha256,
                "--width-mm",
                "100",
                "--chest-mm",
                "100",
                "--length-mm",
                "100",
                "--armhole-mm",
                "0",
                "--armhole-inset-mm",
                "0",
                "--rib-courses",
                "0",
                "--created",
                "2026-11-27T12:00",
                "--out",
                out_dir.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();

        let selected_source = args.options.remove("material-sha256").unwrap();
        assert_eq!(
            cmd_knit(&args).unwrap_err().code,
            RefusalCode::UnqualifiedProcess
        );
        assert!(!out_dir.exists());
        args.options
            .insert("material-sha256".to_string(), selected_source);

        args.options.insert("wales".to_string(), "22".to_string());
        assert_eq!(
            cmd_knit(&args).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        assert!(!out_dir.exists());
        args.options.remove("wales");

        let (result, code, _) = cmd_knit(&args).unwrap();
        assert_eq!(code, 0);
        assert_eq!(result["machine_executable"], false);
        assert!(result["backend_export_refusal"].is_object());
        assert!(!out_dir.join("program.kc").exists());
        let pattern_path = out_dir.join("pattern.knit.json");
        let doc: KnitDoc = serde_json::from_slice(&std::fs::read(&pattern_path).unwrap()).unwrap();
        assert_eq!(doc.gauge.wales_per_100mm, 20.0);
        assert_eq!(doc.gauge.courses_per_100mm, 25.0);
        assert_eq!(doc.gauge.thickness_mm, Some(1.6));
        assert!(doc.gauge.relaxed_after_wash);
        assert_eq!(doc.metadata["material_sha256"], material_sha256);
        assert_eq!(doc.metadata["yarn_lot_id"], "synthetic-lot");
        assert!(
            doc.stitches
                .iter()
                .all(|st| st.cam_stitch_setting.setting == 6.0)
        );

        let verify = parse(
            &[
                "knit",
                "verify",
                pattern_path.to_str().unwrap(),
                "--machine",
                "kniterate-7e",
                "--qual",
                qual_path.to_str().unwrap(),
                "--material-sha256",
                &material_sha256,
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(cmd_knit(&verify).unwrap().1, 0);
        let mut wrong_source = verify;
        wrong_source
            .options
            .insert("material-sha256".to_string(), "b".repeat(64));
        assert_eq!(
            cmd_knit(&wrong_source).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
    }

    #[test]
    fn knit_and_dye_cli_refuse_directories_and_unix_fifos_without_blocking() {
        let root = fresh_dir("nonregular");
        let dir_str = root.to_str().unwrap();

        let knit_dir = parse(&["knit", "verify", dir_str].map(String::from)).unwrap();
        assert_eq!(
            cmd_knit(&knit_dir).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        let dye_dir = parse(&["dye", "verify", dir_str].map(String::from)).unwrap();
        assert_eq!(
            cmd_dye(&dye_dir).unwrap_err().code,
            RefusalCode::InvalidDocument
        );

        #[cfg(unix)]
        {
            let fifo = root.join("knit.fifo");
            let created = std::process::Command::new("mkfifo")
                .arg(&fifo)
                .status()
                .is_ok_and(|s| s.success());
            assert!(created, "mkfifo should create test FIFO");
            let fifo_str = fifo.to_str().unwrap();
            let knit_fifo = parse(&["knit", "verify", fifo_str].map(String::from)).unwrap();
            assert_eq!(
                cmd_knit(&knit_fifo).unwrap_err().code,
                RefusalCode::InvalidDocument
            );
            let dye_fifo = parse(&["dye", "verify", fifo_str].map(String::from)).unwrap();
            assert_eq!(
                cmd_dye(&dye_fifo).unwrap_err().code,
                RefusalCode::InvalidDocument
            );
        }
    }
}
