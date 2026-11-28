//! `ftx knit` and `ftx dye` subcommands (`FTX-K17`):
//! - `ftx knit swatch` — hard-capped `CalibrationSwatchOnly` kit (`.k`, AYAB, CYC SVG/PDF, cert, template)
//! - `ftx knit draft` — measured flat panel or shaped torso tube with child-safety gates
//! - `ftx knit verify` — 7-check V-bed VM & child-safety verifier for any `ftx-knit/1` document
//! - `ftx knit export` — recompile and export a saved document with its current qualification
//! - `ftx knit pullover` — four-piece garment with checked manual seam assembly
//! - `ftx knit shape` — source-bound annular shell to verified knit program
//! - `ftx knit poincloux` — published sliding-swatch comparison or explicitly unvalidated legacy surrogate
//! - `ftx dye optimize` — 4-stage radiometric KM + Sobol-CVaR Bertsekas L-BFGS + 5-regime Pareto
//! - `ftx dye verify` — physical dye-rig Capstan/dwell/Tg/accumulator/fastness verifier + `.csv`/`.gcode`/cert

mod body_draft;
mod shape;

use std::path::{Path, PathBuf};

use ftx_body::knit_shell::{
    AnnyAnatomicalShell, AnnyKnitShell, anatomical_shell_json, body_mesh_sha256,
};
use ftx_body::{BodyFrames, clothing_skin_with_hands, skin_mesh};
use ftx_core::receipt::sha256_hex;
use ftx_core::shaping::{EaseProfile, EaseShellMesh, KnitShellRegion, ShapingSurface};
use ftx_core::{Refusal, RefusalCode, TargetRaster};
use ftx_drape::dye::{
    CapillaryTransportConfig, DriftParameters, DyeInverseConfig, DyeOpticsModel,
    DyeUncertaintyContext, KnitStitchMap, StitchArcMapping, optimize_yarn_dye_program_with_context,
    rank_five_regime_pareto, srgb_u8_to_xyz_d65,
};
use ftx_drape::knit::{
    KnitDrapeOptions, PoinclouxParams, audit_solver_knit_patch, drape_knit_doc,
    poincloux_uniaxial_curve,
};
use ftx_export::dye::{export_exp4_bleed_swatch_kit, verify_and_export_dye_program_with_receipt};
use ftx_export::knit::{
    check_hand_knit_pdf, check_hand_knit_svg, export_brother_ayab, export_calibration_swatch_kit,
    export_hand_knit_document, export_knit_document, export_kniterate_kc, export_knitout_v2,
    export_seamed_pullover, export_tier_b_vendor_handoff, refuse_proprietary_binary_target,
    verify_knitout_v2_readback,
};
use ftx_pattern::dye::{
    DyeMachineProfile, DyePlanUncertaintyReceipt, DyeingRegime, YarnDyeProgram,
    YarnDyeingQualification,
};
use ftx_pattern::knit::{
    AutoKnitConfig, Bed, BranchedPulloverSpec, CableGroup, CalibrationSwatchCaps,
    DrapeUncertaintyReceipt, GaugeSpec, KnitDoc, KnitSafetyContext, KnitSwatchQualification,
    MachineBackendKind, MachineProfile, ParametricPanelSpec, RaglanPulloverSpec,
    SeamedPulloverSpec, WearerAgeClass, apply_cable_crossings, compile_knit_doc,
    draft_autoknit_from_shell, draft_branched_raglan_pullover, draft_seamed_pullover,
    draft_seamless_raglan_tube, draft_short_row_neckline_panel,
};
use ftx_pattern::{material_catalog::ResolvedMaterial, textile_import::resolve_fabric};
use ftx_trace::raster::load_target_raster_with_limit;
use serde_json::{Value, json};

use super::{
    Args, bad_input, body_source, current_measurements, export, known_options, obj,
    pose_fitted_body, read_json, required, reserve_drape_output, reserve_output, workspace,
    write_drape_output,
};

const MAX_KNIT_FILE_BYTES: u64 = 64 * 1024 * 1024;
const SWATCH_MATERIAL_PLACEHOLDER: &str = "unmeasured-yarn-lot-placeholder";

/// Boolean flags accepted by `ftx knit` and `ftx dye` without a following value.
pub(super) const BOOLEAN_OPTIONS: &[&str] = &[
    "circular",
    "has-drawstring",
    "allow-proprietary-binary",
    "strict-knitout",
    "allow-negative-ease",
];

fn parse_machine(id: &str) -> Result<MachineProfile, Refusal> {
    match id {
        "kniterate-7e" | "kniterate" => Ok(MachineProfile::kniterate_7e()),
        "brother-kh930-ayab" | "brother-kh930" | "kh930" => {
            Ok(MachineProfile::brother_kh930_ayab())
        }
        "brother-kh270-ayab" | "brother-kh270" | "kh270" => {
            Ok(MachineProfile::brother_kh270_ayab())
        }
        "generic-vbed-12e" => Ok(MachineProfile::generic_vbed_12e()),
        "generic-slider-12e" => Ok(MachineProfile::generic_slider_12e()),
        "shima-seiki-12e-handoff" | "shima-12e" | "shima" => {
            Ok(MachineProfile::shima_seiki_12e_handoff())
        }
        "stoll-12e-handoff" | "stoll-12e" | "stoll" => Ok(MachineProfile::stoll_12e_handoff()),
        "hand-knit" | "hand" => Ok(MachineProfile::hand_knit(4.0)),
        _ => Err(bad_input(format!(
            "unknown --machine '{id}' (expected kniterate-7e, brother-kh930, brother-kh270, generic-vbed-12e, generic-slider-12e, shima-12e, stoll-12e, or hand-knit)"
        ))),
    }
}

fn default_cam_setting(profile: &MachineProfile) -> f64 {
    let mid = 0.5 * (profile.cam_setting_range[0] + profile.cam_setting_range[1]);
    if profile.backend_kind == MachineBackendKind::KniterateKc {
        mid.floor()
    } else {
        mid
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

/// Dispatches `ftx knit <swatch|draft|measured|pullover|shape|verify|export|shell|poincloux> ...`.
pub(super) fn cmd_knit(args: &Args) -> Result<(Value, i32, String), Refusal> {
    match args.positional.get(1).map(String::as_str) {
        Some("swatch") => cmd_knit_swatch(args),
        Some("draft") => cmd_knit_draft(args),
        Some("measured") => body_draft::cmd_knit_measured(args),
        Some("pullover") => cmd_knit_pullover(args),
        Some("shape") if args.positional.len() > 2 || args.options.contains_key("config") => {
            shape::cmd_knit_shape(args)
        }
        Some("shape") => cmd_knit_shape(args),
        Some("verify") => cmd_knit_verify(args),
        Some("export") => cmd_knit_export(args),
        Some("shell") => crate::knit_shell::cmd_knit_shell(args),
        Some("poincloux") => cmd_knit_poincloux(args),
        _ => Err(bad_input(
            "expected `ftx knit swatch|draft|measured|pullover|shape|verify|export|shell|poincloux`",
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
        // Choose representable defaults; explicit --cams values above remain unchanged.
        None if profile.backend_kind == MachineBackendKind::KniterateKc => vec![5.0, 8.0],
        None => [0.35, 0.55]
            .into_iter()
            .map(|fraction| {
                let setting = profile.cam_setting_range[0]
                    + fraction * (profile.cam_setting_range[1] - profile.cam_setting_range[0]);
                (setting * 10.0).round() / 10.0
            })
            .collect(),
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

    if let Some(kc) = &kc {
        write_file(&out_dir, "swatch.kc", kc.kc_text.as_bytes())?;
        let verification = serde_json::to_vec_pretty(&kc.verification)
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
        write_file(&out_dir, "kcode_verification.json", &verification)?;
        files.push("swatch.kc".to_string());
        files.push("kcode_verification.json".to_string());
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
        "kcode_verification": kc.as_ref().map(|bundle| &bundle.verification),
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

fn body_readings_mm(
    description: &serde_json::Map<String, Value>,
) -> Option<&serde_json::Map<String, Value>> {
    description.get("readings_mm").and_then(Value::as_object)
}

fn reading_or(readings: Option<&serde_json::Map<String, Value>>, key: &str, fallback: f64) -> f64 {
    readings
        .and_then(|m| m.get(key))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(fallback)
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
            "measurements",
            "body-fit",
            "model",
            "pose",
            "skinning",
            "thumb-pose",
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
            "shoulder-short-row-pairs",
            "rib-courses",
            "yoke-mm",
            "collar-rib",
            "sleeve-cuff-mm",
            "upper-arm-mm",
            "sleeve-length-mm",
            "cuff-rib",
            "cables",
            "qual",
            "material-sha256",
            "drape-receipt",
            "created",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx knit draft --age-class infant|child|adult --out NEW_DIR [--style panel|tube|pullover|rib] [--measurements M] [--body-fit REPORT] [--machine ID] [--qual SWATCH.json --material-sha256 SOURCE_DIGEST]",
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
        qual.as_ref()
            .map_or_else(|| default_cam_setting(&profile), |q| q.cam_stitch_setting),
    )?;
    let has_drawstring = args.options.contains_key("has-drawstring");
    let shoulder_short_row_pairs = opt_u32(args, "shoulder-short-row-pairs", 0)?;
    if shoulder_short_row_pairs != 0 && style != "panel" {
        return Err(bad_input(
            "--shoulder-short-row-pairs requires --style panel",
        ));
    }

    if style != "pullover"
        && [
            "sleeve-cuff-mm",
            "upper-arm-mm",
            "sleeve-length-mm",
            "cuff-rib",
        ]
        .iter()
        .any(|key| args.options.contains_key(*key))
    {
        return Err(bad_input(
            "sleeve dimensions and --cuff-rib require --style pullover",
        ));
    }
    let prepared_body =
        if args.options.contains_key("measurements") || args.options.contains_key("body-fit") {
            let meas = args
                .options
                .get("measurements")
                .map(|p| read_json(p).and_then(|(v, _)| current_measurements(v)))
                .transpose()?;
            Some(body_source::prepare_bound(args, meas.as_ref(), None)?)
        } else {
            None
        };
    let readings = prepared_body
        .as_ref()
        .and_then(|b| body_readings_mm(&b.description));
    let body_chest_girth_mm = reading_or(readings, "chest", 560.0);
    let body_stature_mm = reading_or(readings, "stature", 950.0);
    let body_torso_len_mm = reading_or(
        readings,
        "back_neck_to_waist",
        (0.25 * body_stature_mm).max(160.0),
    );
    let body_arm_len_mm = reading_or(readings, "arm_length", 200.0);
    let body_upper_arm_mm = reading_or(readings, "upper_arm", 220.0);
    let body_wrist_mm = reading_or(readings, "wrist", 150.0);

    let mut doc = match style {
        "panel" => {
            let panel_chest_default = if readings.is_some() {
                0.5 * body_chest_girth_mm + 15.0
            } else {
                280.0
            };
            let panel_hem_default = if readings.is_some() {
                (panel_chest_default - 20.0).max(120.0)
            } else {
                260.0
            };
            let panel_len_default = if readings.is_some() {
                body_torso_len_mm
            } else {
                320.0
            };
            draft_short_row_neckline_panel(
                &ParametricPanelSpec {
                    name: args
                        .options
                        .get("name")
                        .cloned()
                        .unwrap_or_else(|| "knitted_panel".to_string()),
                    wearer_age_class,
                    hem_width_mm: opt_f64(args, "width-mm", panel_hem_default)?,
                    chest_width_mm: opt_f64(args, "chest-mm", panel_chest_default)?,
                    length_mm: opt_f64(args, "length-mm", panel_len_default)?,
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
                shoulder_short_row_pairs,
            )?
        }
        "tube" | "raglan" | "pullover" | "rib" => {
            let chest_default = if readings.is_some() {
                body_chest_girth_mm + 24.0
            } else {
                560.0
            };
            let neck_default = if readings.is_some() {
                (0.5 * body_chest_girth_mm).clamp(180.0, chest_default - 20.0)
            } else {
                280.0
            };
            let len_default = if readings.is_some() {
                (body_torso_len_mm * 0.65).max(80.0)
            } else {
                200.0
            };
            let default_rib = if style == "rib" { 8 } else { 0 };
            let torso = RaglanPulloverSpec {
                name: args.options.get("name").cloned().unwrap_or_else(|| {
                    match style {
                        "pullover" => "branched_raglan_pullover",
                        "rib" => "ribbed_knit_tube",
                        _ => "shaped_torso_tube",
                    }
                    .to_string()
                }),
                wearer_age_class,
                neck_girth_mm: opt_f64(args, "neck-mm", neck_default)?,
                chest_girth_mm: opt_f64(args, "chest-mm", chest_default)?,
                yoke_depth_mm: opt_f64(args, "yoke-mm", 120.0)?,
                body_length_below_arm_mm: opt_f64(args, "length-mm", len_default)?,
                collar_rib_courses: opt_u32(args, "collar-rib", default_rib)?,
                hem_rib_courses: opt_u32(args, "rib-courses", default_rib)?,
                cam_setting: cam,
                yarn_id,
                carrier_id: profile.carrier_ids[0].clone(),
                has_hood_or_neck_drawstring: has_drawstring,
            };
            if style == "pullover" {
                let missing_carriers =
                    || bad_input("a branched pullover requires three available yarn carriers");
                draft_branched_raglan_pullover(
                    &BranchedPulloverSpec {
                        torso,
                        sleeve_cuff_girth_mm: opt_f64(
                            args,
                            "sleeve-cuff-mm",
                            if readings.is_some() {
                                body_wrist_mm + 12.0
                            } else {
                                150.0
                            },
                        )?,
                        sleeve_upper_arm_girth_mm: opt_f64(
                            args,
                            "upper-arm-mm",
                            if readings.is_some() {
                                body_upper_arm_mm + 16.0
                            } else {
                                220.0
                            },
                        )?,
                        sleeve_length_mm: opt_f64(
                            args,
                            "sleeve-length-mm",
                            if readings.is_some() {
                                body_arm_len_mm
                            } else {
                                200.0
                            },
                        )?,
                        cuff_rib_courses: opt_u32(args, "cuff-rib", 0)?,
                        left_sleeve_carrier: profile
                            .carrier_ids
                            .get(1)
                            .ok_or_else(missing_carriers)?
                            .clone(),
                        right_sleeve_carrier: profile
                            .carrier_ids
                            .get(2)
                            .ok_or_else(missing_carriers)?
                            .clone(),
                    },
                    &gauge,
                    &profile,
                )?
            } else {
                draft_seamless_raglan_tube(&torso, &gauge, &profile)?
            }
        }
        _ => {
            return Err(bad_input(format!(
                "unknown --style '{style}' (expected panel, tube, pullover, or rib; raglan is a legacy tube alias)"
            )));
        }
    };

    if let Some(groups) =
        read_optional_knit_json::<Vec<CableGroup>>(args, "cables", "cable groups")?
    {
        doc = apply_cable_crossings(&doc, &groups)?;
    }

    let receipt: Option<DrapeUncertaintyReceipt> =
        read_optional_knit_json(args, "drape-receipt", "knit drape receipt")?;
    if let Some(material_sha256) = expected_material_sha256 {
        doc.metadata
            .insert("material_sha256".to_string(), material_sha256.to_string());
    }
    if let Some(q) = &qual {
        doc.metadata
            .insert("yarn_lot_id".to_string(), q.yarn_lot_id.clone());
    }
    let created = match args.options.get("created") {
        Some(c) => c.clone(),
        None => export::utc_minute()?,
    };
    let bundle = export_knit_document(
        &doc,
        &profile,
        qual.as_ref(),
        expected_material_sha256,
        receipt.as_ref(),
        &created,
    )?;

    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit draft` requires --out NEW_DIR"))?,
    );
    reserve_output(&out_dir, "knit draft")?;
    for (name, bytes) in &bundle.files {
        write_file(&out_dir, name, bytes)?;
    }

    let conversion_notice = bundle
        .backend_export_refusal
        .as_ref()
        .map_or_else(String::new, |refusal| {
            format!("\nMachine conversion required: {}", refusal.message)
        });
    let result = json!({
        "style": style,
        "machine": profile.id,
        "wearer_age_class": doc.wearer_age_class,
        "stitches": doc.stitches.len(),
        "transfers": bundle.compiled.vm_report.total_transfers,
        "kcode_verification": bundle.kcode_verification,
        "max_float_mm": bundle.compiled.vm_report.max_float_mm,
        "files": bundle.files.keys().collect::<Vec<_>>(),
        "metadata": doc.metadata,
        "cert": bundle.compiled.cert,
        "backend_export_refusal": bundle.backend_export_refusal,
        "body": prepared_body.as_ref().map(|b| &b.description),
        "machine_executable": false,
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    let text = format!(
        "Drafted & verified {style} knit ({} stitches, {} transfers) for {} -> {}{conversion_notice}",
        doc.stitches.len(),
        bundle.compiled.vm_report.total_transfers,
        profile.id,
        out_dir.display()
    );
    Ok((result, 0, text))
}

fn parse_ease_profile(args: &Args) -> Result<EaseProfile, Refusal> {
    let mut ease = match args.options.get("ease").map(String::as_str) {
        None | Some("relaxed" | "relaxed_pullover") => EaseProfile::relaxed_pullover(),
        Some("fitted") => EaseProfile {
            torso_radial_mm: 4.0,
            limb_radial_mm: 3.0,
            min_clearance_mm: 1.0,
            allow_negative_ease: false,
        },
        Some(path) => {
            let txt = read_utf8(Path::new(path), "knit ease profile")?;
            serde_json::from_str(&txt)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?
        }
    };
    if args.options.contains_key("allow-negative-ease") {
        ease.allow_negative_ease = true;
    }
    Ok(ease)
}

fn parse_shell_region(raw: &str) -> Result<KnitShellRegion, Refusal> {
    match raw {
        "torso" => Ok(KnitShellRegion::Torso),
        "left_sleeve" | "left-sleeve" => Ok(KnitShellRegion::LeftSleeve),
        "right_sleeve" | "right-sleeve" => Ok(KnitShellRegion::RightSleeve),
        "full_upper_body" | "full-upper-body" => Ok(KnitShellRegion::FullUpperBody),
        "tube" => Ok(KnitShellRegion::Tube),
        _ => Err(bad_input(format!(
            "invalid --region '{raw}' (expected torso, left_sleeve, right_sleeve, full_upper_body, or tube)"
        ))),
    }
}

fn cmd_knit_shape(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit shape",
        &[
            "measurements",
            "body-fit",
            "model",
            "pose",
            "skinning",
            "thumb-pose",
            "region",
            "ease",
            "allow-negative-ease",
            "input",
            "shell",
            "qual",
            "material-sha256",
            "drape-receipt",
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
            "max-idt-flips",
            "name",
            "created",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx knit shape --age-class infant|child|adult --out NEW_DIR (--measurements M | --body-fit REPORT | --shell SHELL.json | --input ANATOMY.json) [--region REGION] [--ease EASE.json] [--qual SWATCH.json --material-sha256 SHA]",
    )?;
    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit shape` requires --out NEW_DIR"))?,
    );
    let age_str = args.options.get("age-class").ok_or_else(|| {
        bad_input("`ftx knit shape` requires explicit --age-class infant|child|adult")
    })?;
    let wearer_age_class = parse_age_class(age_str)?;
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("kniterate-7e");
    let profile = parse_machine(machine_id)?;
    if args.options.contains_key("allow-proprietary-binary") {
        refuse_proprietary_binary_target(&profile, "proprietary_binary")?;
    }
    let qual = read_knit_qualification(args)?;
    let expected_material_sha256 = expected_knit_material(args)?;
    let receipt: Option<DrapeUncertaintyReceipt> =
        read_optional_knit_json(args, "drape-receipt", "knit drape receipt")?;

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
        qual.as_ref()
            .map_or_else(|| default_cam_setting(&profile), |q| q.cam_stitch_setting),
    )?;
    let has_hood_or_neck_drawstring = args.options.contains_key("has-drawstring");
    let max_idt_flips = opt_u32(args, "max-idt-flips", 200)? as usize;

    let (shell, source_description) = if let Some(shell_path) = args.options.get("shell") {
        let txt = read_utf8(Path::new(shell_path), "ease shell mesh")?;
        let shell: EaseShellMesh = serde_json::from_str(&txt)
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
        let desc = json!({"kind": "ease_shell_mesh", "shell_sha256": sha256_hex(txt.as_bytes())});
        (shell, desc)
    } else if let Some(input_path) = args.options.get("input") {
        let txt = read_utf8(Path::new(input_path), "anatomical shell request")?;
        let shell_json = anatomical_shell_json(&txt)?;
        let shell: EaseShellMesh = serde_json::from_str(&shell_json)
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
        let desc = json!({"kind": "explicit_anatomy", "source_sha256": sha256_hex(txt.as_bytes())});
        (shell, desc)
    } else if args.options.contains_key("measurements") || args.options.contains_key("body-fit") {
        let region_raw = args
            .options
            .get("region")
            .map(String::as_str)
            .unwrap_or("torso");
        let region = parse_shell_region(region_raw)?;
        let ease = parse_ease_profile(args)?;
        let pose = crate::drape_pose(args)?;
        let skinning = crate::parse_skinning(args)?;
        let thumbs = crate::parse_thumb_pose(args)?;
        let meas = args
            .options
            .get("measurements")
            .map(|p| read_json(p).and_then(|(v, _)| current_measurements(v)))
            .transpose()?;
        let prepared = body_source::prepare_bound(args, meas.as_ref(), None)?;
        let rest = prepared.model.rest(&prepared.params)?;
        let posed = pose_fitted_body(&prepared.model, &rest, &pose, skinning, thumbs)?;
        let full = skin_mesh(&posed.vertices, &prepared.model.data().faces)?;
        let source_body_sha256 = body_mesh_sha256(&full.vertices, &full.triangles)?;
        let shell = if region == KnitShellRegion::Tube {
            let frames = BodyFrames::new(&prepared.model, &posed)?;
            let skin = clothing_skin_with_hands(
                &prepared.model,
                &posed.vertices,
                ftx_body::HandCollider::Wrist,
            )?;
            AnnyKnitShell {
                skin: &skin,
                frames: &frames,
                source_body_sha256,
            }
            .build_ease_shell(region, &ease)?
        } else {
            AnnyAnatomicalShell {
                model: &prepared.model,
                posed: &posed,
                source_body_sha256,
            }
            .build_ease_shell(region, &ease)?
        };
        let desc = json!({
            "kind": "anny_shaping_surface",
            "region": region,
            "ease": ease,
            "body": prepared.description,
        });
        (shell, desc)
    } else {
        return Err(bad_input(
            "`ftx knit shape` requires --measurements/--body-fit, --shell SHELL.json, or --input ANATOMY.json",
        ));
    };

    let cfg = AutoKnitConfig {
        name: args
            .options
            .get("name")
            .cloned()
            .unwrap_or_else(|| "autoknit_shaped_garment".to_string()),
        wearer_age_class,
        yarn_id,
        carrier_id: profile.carrier_ids[0].clone(),
        cam_setting: cam,
        max_idt_flips,
        has_hood_or_neck_drawstring,
    };
    let mut rep = draft_autoknit_from_shell(&shell, &gauge, &profile, &cfg)?;
    if let Some(material_sha256) = expected_material_sha256 {
        rep.doc
            .metadata
            .insert("material_sha256".to_string(), material_sha256.to_string());
    }
    if let Some(q) = &qual {
        rep.doc
            .metadata
            .insert("yarn_lot_id".to_string(), q.yarn_lot_id.clone());
    }
    let created = match args.options.get("created") {
        Some(c) => c.clone(),
        None => export::utc_minute()?,
    };
    let export_bundle =
        if qual.is_some() || receipt.is_some() || !profile.requires_calibration_swatch {
            Some(export_knit_document(
                &rep.doc,
                &profile,
                qual.as_ref(),
                expected_material_sha256,
                receipt.as_ref(),
                &created,
            )?)
        } else {
            None
        };

    let doc_json = serde_json::to_vec_pretty(&rep.doc)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let shell_json = serde_json::to_vec_pretty(&shell)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let shape_report_val = json!({
        "schema": "ftx-autoknit-report/1",
        "pattern_sha256": rep.doc.canonical_sha256()?,
        "source_body_sha256": shell.source_body_sha256,
        "source_shell_sha256": rep.source_shell_sha256,
        "geometry_method": rep.geometry_method,
        "source": source_description,
        "shell_height_mm": rep.shell_height_mm,
        "course_count": rep.course_count,
        "height_residual_mm": rep.height_residual_mm,
        "courses": rep.courses,
        "surface_topology": rep.doc.metadata.get("surface_topology"),
        "course_growth_direction": rep.doc.metadata.get("course_growth_direction"),
        "idt_flips_applied": rep.idt_flips_applied,
        "min_cotangent_weight": rep.min_cotangent_weight,
        "max_abs_girth_residual_mm": rep.max_abs_girth_residual_mm,
        "stitches": rep.doc.stitches.len(),
        "boundary_rings": shell.boundary_rings.iter().map(|r| json!({
            "name": r.name,
            "is_start": r.is_start,
            "perimeter_m": r.perimeter_m,
        })).collect::<Vec<_>>(),
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    let shape_report_bytes = serde_json::to_vec_pretty(&shape_report_val)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;

    reserve_output(&out_dir, "knit shape")?;
    let mut files = Vec::new();
    if let Some(bundle) = &export_bundle {
        for (name, bytes) in &bundle.files {
            write_file(&out_dir, name, bytes)?;
            files.push(name.clone());
        }
    } else {
        write_file(&out_dir, "pattern.knit.json", &doc_json)?;
        files.push("pattern.knit.json".to_string());
    }
    write_file(&out_dir, "shell.json", &shell_json)?;
    files.push("shell.json".to_string());
    write_file(&out_dir, "autoknit_report.json", &shape_report_bytes)?;
    files.push("autoknit_report.json".to_string());

    let result = json!({
        "machine": profile.id,
        "wearer_age_class": rep.doc.wearer_age_class,
        "pattern_sha256": rep.doc.canonical_sha256()?,
        "source_shell_sha256": rep.source_shell_sha256,
        "geometry_method": rep.geometry_method,
        "stitches": rep.doc.stitches.len(),
        "shell_height_mm": rep.shell_height_mm,
        "course_count": rep.course_count,
        "height_residual_mm": rep.height_residual_mm,
        "surface_topology": rep.doc.metadata.get("surface_topology"),
        "course_growth_direction": rep.doc.metadata.get("course_growth_direction"),
        "idt_flips_applied": rep.idt_flips_applied,
        "min_cotangent_weight": rep.min_cotangent_weight,
        "max_abs_girth_residual_mm": rep.max_abs_girth_residual_mm,
        "boundary_rings": shape_report_val["boundary_rings"],
        "exported": export_bundle.is_some(),
        "cert": export_bundle.as_ref().map(|b| &b.compiled.cert),
        "backend_export_refusal": export_bundle.as_ref().and_then(|b| b.backend_export_refusal.as_ref()),
        "files": files,
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    let text = format!(
        "Shaped 3D knit ({} courses, {} stitches, max girth residual {:.2} mm, iDT flips {}) -> {}",
        rep.course_count,
        rep.doc.stitches.len(),
        rep.max_abs_girth_residual_mm,
        rep.idt_flips_applied,
        out_dir.display()
    );
    Ok((result, 0, text))
}

fn cmd_knit_pullover(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit pullover",
        &[
            "gauge",
            "machine",
            "qual",
            "material-sha256",
            "drape-receipts",
            "created",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        3,
        "ftx knit pullover SPEC.json --gauge GAUGE.json --out NEW_DIR [--machine ID] [--qual SWATCH.json --material-sha256 SHA] [--drape-receipts RECEIPTS.json]",
    )?;
    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit pullover` requires --out NEW_DIR"))?,
    );
    let spec: SeamedPulloverSpec = serde_json::from_str(&read_utf8(
        &PathBuf::from(&args.positional[2]),
        "seamed pullover specification",
    )?)
    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let gauge: GaugeSpec = read_optional_knit_json(args, "gauge", "pullover gauge")?
        .ok_or_else(|| bad_input("`ftx knit pullover` requires --gauge GAUGE.json"))?;
    let profile = parse_machine(
        args.options
            .get("machine")
            .map(String::as_str)
            .unwrap_or("hand-knit"),
    )?;
    let qualification = read_knit_qualification(args)?;
    let material_sha256 = expected_knit_material(args)?;
    let receipts =
        read_optional_knit_json(args, "drape-receipts", "per-piece pullover drape receipts")?
            .unwrap_or_default();
    let created = match args.options.get("created") {
        Some(created) => created.clone(),
        None => export::utc_minute()?,
    };
    let draft = draft_seamed_pullover(&spec, &gauge, &profile)?;
    let bundle = export_seamed_pullover(
        &draft,
        &profile,
        qualification.as_ref(),
        material_sha256,
        &receipts,
        &created,
    )?;
    reserve_output(&out_dir, "knit pullover")?;
    for piece in &draft.pieces {
        reserve_output(&out_dir.join(piece.role.as_str()), "pullover piece")?;
    }
    for (name, bytes) in &bundle.files {
        write_file(&out_dir, name, bytes)?;
    }
    let result = json!({
        "machine":profile.id,
        "pieces":bundle.piece_certificates,
        "seam_count":draft.seams.len(),
        "openings":draft.openings,
        "residuals_mm":draft.residuals_mm,
        "backend_export_refusals":bundle.backend_export_refusals,
        "files":bundle.files.keys().collect::<Vec<_>>(),
        "out_dir":out_dir.display().to_string(),
        "assembly":"four separate pieces joined manually",
        "hardware_execution_authorized":false,
        "physical_knitting_validated":false,
        "fit_claimed":false,
    });
    let text = format!(
        "Exported four-piece pullover '{}' into {} ({} artifacts, {} checked seams). Manual assembly required.",
        spec.name,
        out_dir.display(),
        bundle.files.len(),
        draft.seams.len(),
    );
    Ok((result, 0, text))
}

fn knitout_v2_generic_profile() -> MachineProfile {
    let mut p = MachineProfile::kniterate_7e();
    p.id = "knitout-v2".to_string();
    p.name = "Generic V-Bed (CMU Knitout v2)".to_string();
    p.backend_kind = MachineBackendKind::KnitoutV2;
    p
}

fn resolve_export_profile(args: &Args) -> Result<MachineProfile, Refusal> {
    let machine_opt = args.options.get("machine").map(String::as_str);
    let profile = match args.options.get("target").map(String::as_str) {
        None => parse_machine(machine_opt.unwrap_or("kniterate-7e"))?,
        Some("shima-dat" | "dat") => {
            refuse_proprietary_binary_target(&MachineProfile::shima_seiki_12e_handoff(), ".dat")?;
            unreachable!("refuse_proprietary_binary_target always errors on Tier-B")
        }
        Some("stoll-sintral" | "sintral") => {
            refuse_proprietary_binary_target(&MachineProfile::stoll_12e_handoff(), "SINTRAL")?;
            unreachable!("refuse_proprietary_binary_target always errors on Tier-B")
        }
        Some("knitout" | "knitout-v2") => match machine_opt {
            Some(m) => parse_machine(m)?,
            None => knitout_v2_generic_profile(),
        },
        Some("kniterate" | "kniterate-7e") => {
            let p = parse_machine(machine_opt.unwrap_or("kniterate-7e"))?;
            if p.backend_kind != MachineBackendKind::KniterateKc {
                return Err(bad_input(format!(
                    "--machine '{}' conflicts with --target kniterate",
                    p.id
                )));
            }
            p
        }
        Some("ayab" | "brother-kh930" | "brother-kh930-ayab") => {
            let p = parse_machine(machine_opt.unwrap_or("brother-kh930"))?;
            if p.backend_kind != MachineBackendKind::BrotherAyab {
                return Err(bad_input(format!(
                    "--machine '{}' conflicts with --target ayab",
                    p.id
                )));
            }
            p
        }
        Some("brother-kh270" | "brother-kh270-ayab") => {
            let p = parse_machine(machine_opt.unwrap_or("brother-kh270"))?;
            if p.id != "brother-kh270-ayab" {
                return Err(bad_input(format!(
                    "--machine '{}' conflicts with --target brother-kh270",
                    p.id
                )));
            }
            p
        }
        Some("hand-pdf" | "hand-knit" | "hand") => {
            let p = parse_machine(machine_opt.unwrap_or("hand-knit"))?;
            if p.backend_kind != MachineBackendKind::HandKnitChart {
                return Err(bad_input(format!(
                    "--machine '{}' conflicts with --target hand-pdf",
                    p.id
                )));
            }
            p
        }
        Some("shima-handoff" | "shima-seiki-handoff" | "shima-12e" | "shima") => {
            let p = parse_machine(machine_opt.unwrap_or("shima-12e"))?;
            if p.backend_kind != MachineBackendKind::ShimaSeikiHandoff {
                return Err(bad_input(format!(
                    "--machine '{}' conflicts with --target shima-handoff",
                    p.id
                )));
            }
            p
        }
        Some("stoll-handoff" | "stoll-12e" | "stoll") => {
            let p = parse_machine(machine_opt.unwrap_or("stoll-12e"))?;
            if p.backend_kind != MachineBackendKind::StollHandoff {
                return Err(bad_input(format!(
                    "--machine '{}' conflicts with --target stoll-handoff",
                    p.id
                )));
            }
            p
        }
        Some(other) => {
            return Err(bad_input(format!(
                "unknown --target '{other}' (expected knitout, kniterate, ayab, hand-pdf, shima-handoff, stoll-handoff, shima-dat, or stoll-sintral)"
            )));
        }
    };
    if args.options.contains_key("allow-proprietary-binary") {
        refuse_proprietary_binary_target(&profile, "proprietary_binary")?;
    }
    Ok(profile)
}

fn cmd_knit_export(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit export",
        &[
            "machine",
            "target",
            "allow-proprietary-binary",
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
        "ftx knit export PATTERN.knit.json --out NEW_DIR [--target knitout|kniterate|ayab|hand-pdf|shima-handoff|stoll-handoff] [--machine ID] [--qual SWATCH.json] [--material-sha256 SHA] [--drape-receipt RECEIPT.json] [--created YYYY-MM-DDTHH:MM]",
    )?;
    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx knit export` requires --out NEW_DIR"))?,
    );
    let doc: KnitDoc = serde_json::from_str(&read_utf8(
        &PathBuf::from(&args.positional[2]),
        "knit pattern document",
    )?)
    .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let profile = resolve_export_profile(args)?;
    let qualification: Option<KnitSwatchQualification> =
        read_optional_knit_json(args, "qual", "knit swatch qualification")?;
    let expected_material_sha256 = expected_knit_material(args)?;
    let receipt: Option<DrapeUncertaintyReceipt> =
        read_optional_knit_json(args, "drape-receipt", "knit drape receipt")?;
    let created = match args.options.get("created") {
        Some(created) => created.clone(),
        None => export::utc_minute()?,
    };
    let bundle = export_knit_document(
        &doc,
        &profile,
        qualification.as_ref(),
        expected_material_sha256,
        receipt.as_ref(),
        &created,
    )?;
    // Validation, all target exporters and read-back checks finish before creating a directory.
    reserve_output(&out_dir, "knit export")?;
    for (name, bytes) in &bundle.files {
        write_file(&out_dir, name, bytes)?;
    }
    let gauge_assumption_disclosed =
        profile.backend_kind == MachineBackendKind::HandKnitChart && qualification.is_none();
    let result = json!({
        "machine": profile.id,
        "target": args.options.get("target").cloned().unwrap_or_else(|| profile.id.clone()),
        "pattern_sha256": doc.canonical_sha256()?,
        "stitches": doc.stitches.len(),
        "vm_report": bundle.compiled.vm_report,
        "kcode_verification": bundle.kcode_verification,
        "cert": bundle.compiled.cert,
        "backend_export_refusal": bundle.backend_export_refusal,
        "gauge_assumption_disclosed": gauge_assumption_disclosed,
        "machine_executable": false,
        "files": bundle.files.keys().collect::<Vec<_>>(),
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    let conversion_notice = bundle
        .backend_export_refusal
        .as_ref()
        .map_or_else(String::new, |refusal| {
            format!("\nMachine conversion required: {}", refusal.message)
        });
    let text = format!(
        "Exported '{}' for {} into {} ({} verified artifacts){conversion_notice}",
        doc.name,
        profile.id,
        out_dir.display(),
        bundle.files.len(),
    );
    Ok((result, 0, text))
}

fn read_optional_knit_json<T: serde::de::DeserializeOwned>(
    args: &Args,
    option: &str,
    label: &str,
) -> Result<Option<T>, Refusal> {
    args.options
        .get(option)
        .map(|path| {
            serde_json::from_str(&read_utf8(&PathBuf::from(path), label)?)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))
        })
        .transpose()
}

fn cmd_knit_verify(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit verify",
        &[
            "machine",
            "age-class",
            "qual",
            "material-sha256",
            "strict-knitout",
        ],
    )?;
    no_extra_positional(
        args,
        3,
        "ftx knit verify PATTERN.knit.json [--machine ID] [--age-class CLASS] [--strict-knitout] [--qual SWATCH.json --material-sha256 SOURCE_DIGEST] [--json]",
    )?;
    let doc_path = PathBuf::from(&args.positional[2]);
    let txt = read_utf8(&doc_path, "knit pattern document")?;
    let machine_id = args
        .options
        .get("machine")
        .map(String::as_str)
        .unwrap_or("kniterate-7e");
    let profile = parse_machine(machine_id)?;
    let strict_knitout = args.options.contains_key("strict-knitout");

    if txt.trim_start().starts_with(";!knitout-2") {
        let wearer_age_class = match args.options.get("age-class") {
            Some(raw) => parse_age_class(raw)?,
            None => WearerAgeClass::Adult,
        };
        let ctx = KnitSafetyContext {
            wearer_age_class,
            blocked_wale_pitch_mm: profile.needle_pitch_mm,
            source_doc: None,
            speed_fraction: if wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
                Some(CalibrationSwatchCaps::MAX_SPEED_FRACTION)
            } else {
                None
            },
            takedown_fraction: if wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
                Some(CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION)
            } else {
                None
            },
        };
        let (_program, vm_report) = verify_knitout_v2_readback(&txt, &profile, &ctx)?;
        let result = json!({
            "verified": true,
            "strict_knitout": true,
            "wearer_age_class": wearer_age_class,
            "vm_report": vm_report,
            "hardware_execution_authorized": false,
            "physical_knitting_validated": false,
            "fit_claimed": false,
        });
        let text = format!(
            "Verified Knitout v2 stream for {}: {} ops, {} transfers, max float {:.2} mm (hardware_execution_authorized=false)",
            profile.id, vm_report.op_count, vm_report.total_transfers, vm_report.max_float_mm
        );
        return Ok((result, 0, text));
    }

    let doc: KnitDoc = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let qual = read_knit_qualification(args)?;
    let compiled = compile_knit_doc(
        &doc,
        &profile,
        qual.as_ref(),
        expected_knit_material(args)?,
        None,
    )?;
    let knitout = export_knitout_v2(&compiled)?;
    if strict_knitout {
        let ctx = KnitSafetyContext {
            wearer_age_class: doc.wearer_age_class,
            blocked_wale_pitch_mm: doc.gauge.blocked_wale_pitch_mm(),
            source_doc: Some(&doc),
            speed_fraction: if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
                Some(CalibrationSwatchCaps::MAX_SPEED_FRACTION)
            } else {
                None
            },
            takedown_fraction: if doc.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
                Some(CalibrationSwatchCaps::MAX_TAKEDOWN_FRACTION)
            } else {
                None
            },
        };
        let (_reparsed, readback_report) =
            verify_knitout_v2_readback(&knitout.knitout_text, &profile, &ctx)?;
        if readback_report.program_sha256 != compiled.vm_report.program_sha256 {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                "strict Knitout v2 read-back digest disagreed with compiled VM report",
            ));
        }
    }
    let result = json!({
        "verified": true,
        "strict_knitout": strict_knitout,
        "vm_report": compiled.vm_report,
        "cert": compiled.cert,
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
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

fn validate_drape_material_binding(
    doc: &KnitDoc,
    material: &ResolvedMaterial,
    qualification: Option<&KnitSwatchQualification>,
    expected_sha256: Option<&str>,
) -> Result<(), Refusal> {
    for (source, digest) in [
        ("--material-sha256", expected_sha256),
        (
            "KnitDoc material_sha256",
            doc.metadata.get("material_sha256").map(String::as_str),
        ),
    ] {
        if let Some(digest) = digest
            && digest != material.sha256()
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("{source} disagrees with the material selected by --fabric"),
            )
            .repair("select the exact source material used to draft this garment"));
        }
    }
    let Some(qualification) = qualification else {
        return Ok(());
    };
    let family = &doc.stitches[0].cam_stitch_setting.machine_family;
    let profile = if family.starts_with("hand-knit-") {
        MachineProfile::hand_knit(doc.gauge.needle_pitch_mm)
    } else {
        parse_machine(family)?
    };
    qualification.validate_for_production(material.sha256(), &profile.canonical_sha256()?)?;
    if !doc.gauge.relaxed_after_wash {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "supplied --qual describes washed and blocked gauge, but the KnitDoc does not",
        )
        .repair(
            "redraft with the selected swatch's relaxed gauge before using its qualification",
        ));
    }
    for (field, actual, qualified) in [
        (
            "needle_pitch_mm",
            doc.gauge.needle_pitch_mm,
            profile.needle_pitch_mm,
        ),
        (
            "wales_per_100mm",
            doc.gauge.wales_per_100mm,
            qualification.wales_per_100mm,
        ),
        (
            "courses_per_100mm",
            doc.gauge.courses_per_100mm,
            qualification.courses_per_100mm,
        ),
    ] {
        if actual != qualified {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!("KnitDoc {field} ({actual}) disagrees with supplied --qual ({qualified})"),
            )
            .repair("use the gauge and machine recorded for the selected material's swatch"));
        }
    }
    if doc
        .gauge
        .thickness_mm
        .is_some_and(|thickness| thickness != qualification.thickness_mm)
        || doc.stitches.iter().any(|stitch| {
            stitch.cam_stitch_setting.setting != qualification.cam_stitch_setting
                || stitch.cam_stitch_setting.machine_family != profile.id
        })
        || doc
            .metadata
            .get("yarn_lot_id")
            .is_some_and(|lot| lot != &qualification.yarn_lot_id)
    {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "KnitDoc thickness, stitch-cam process, or retained yarn lot disagrees with supplied --qual",
        )
        .repair("retain the exact swatch process and yarn lot, or qualify the changed process"));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) fn cmd_drape_knit(
    args: &Args,
    pattern_path: &str,
    pat: &Value,
    opts: &ftx_drape::run::DrapeOptions,
    pose: ftx_body::Pose,
    thumbs: Option<ftx_body::ThumbPose>,
    skinning: ftx_body::SkinningMode,
    hands: ftx_body::HandCollider,
    expected_model_sha256: Option<&str>,
) -> Result<(Value, i32, String), Refusal> {
    let doc: KnitDoc = serde_json::from_value(pat.clone())
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, format!("knit pattern: {e}")))?;
    doc.validate()?;

    let audit_zone = args.options.get("audit-patch").map(String::as_str);
    if let Some(zone) = audit_zone
        && zone.trim().is_empty()
    {
        return Err(bad_input(
            "--audit-patch requires a non-empty supported zone (all, boundary, or selvedge)",
        ));
    }

    // Freeze the existing whole-fabric selection without adding woven fields to KnitDoc.
    // A gauge qualification never supplies or upgrades the constitutive material model.
    let mut material_pattern = json!({});
    let selected_fabric = super::fabric::load(args)?
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::UnqualifiedProcess,
                "ftx-knit/1 drapes require --fabric with a source-bearing coupled knit material",
            )
            .repair("supply a fabric.material selection containing the constitutive law, mass, thickness, and active contact properties")
        })?
        .apply(&mut material_pattern)?;
    let material = resolve_fabric(&material_pattern["fabric"])?
        .ok_or_else(|| {
            Refusal::new(
                RefusalCode::UnqualifiedProcess,
                "a legacy scalar --fabric does not identify a coupled knit material",
            )
            .repair("select a source-bearing fabric.material record with knit_response; estimates require explicit allow_estimates in the selection")
        })?;
    if material.coupled_knit()?.is_none() {
        return Err(Refusal::new(
            RefusalCode::UnqualifiedProcess,
            "selected --fabric has no coupled knit constitutive law",
        )
        .repair("select a material record with an independently sourced or explicitly estimated knit_response"));
    }
    let qual = read_knit_qualification(args)?;
    validate_drape_material_binding(
        &doc,
        &material,
        qual.as_ref(),
        expected_knit_material(args)?,
    )?;
    // Resolve all active mechanical/contact capabilities before reserving an output directory.
    ftx_drape::material::resolve(&ftx_pattern::Pattern::new(material_pattern), opts)?;
    if doc.metadata.get("pathway").map(String::as_str)
        == Some("autoknit_annular_intrinsic_harmonic")
        && (doc.metadata.get("surface_topology").map(String::as_str) != Some("torso_tube")
            || !matches!(
                doc.metadata
                    .get("course_growth_direction")
                    .map(String::as_str),
                Some("upward" | "downward")
            ))
    {
        return Err(Refusal::new(
            RefusalCode::UnqualifiedProcess,
            "source annulus has no supported torso placement; generic tubes and limb shells are not torso garments",
        )
        .repair("use an explicit hem/neck source shell with unambiguous vertical growth for torso draping; other annuli remain available for shape and knitting export"));
    }

    let meas_path = required(args, "measurements")?;
    let (meas, meas_bytes) = read_json(meas_path)?;
    let meas = current_measurements(meas)?;
    let out = PathBuf::from(required(args, "out")?);
    reserve_drape_output(&out)?;

    let body_source::PreparedBody {
        model,
        params,
        description,
        source_bytes,
    } = body_source::prepare_bound(args, Some(&meas), expected_model_sha256)?;
    let rest = model.rest(&params)?;
    let posed = pose_fitted_body(&model, &rest, &pose, skinning, thumbs)?;
    let skin = clothing_skin_with_hands(&model, &posed.vertices, hands)?;
    let full = skin_mesh(&posed.vertices, &model.data().faces)?;
    let frames = BodyFrames::new(&model, &posed)?;
    let posed_frames = ftx_drape::scene::PosedFrames::new(&frames, pose.arm_pose);
    let body = ftx_drape::run::DrapeBody {
        frames: &posed_frames,
        collision: (&skin.vertices, &skin.triangles),
        display: (&full.vertices, &full.triangles),
        arm_deg: pose.arm_abduction_deg.unwrap_or(ftx_body::Pose::A_POSE_DEG),
        arm_pose: pose.arm_pose,
    };

    let started = std::time::Instant::now();
    let drape_res = drape_knit_doc(
        &doc,
        &body,
        &KnitDrapeOptions {
            drape_opts: *opts,
            material: material.clone(),
        },
    )?;

    let patch_audit = if let Some(zone) = audit_zone {
        Some(audit_solver_knit_patch(
            &drape_res.solver,
            zone,
            &material,
            doc.gauge.blocked_wale_pitch_mm() * 0.001,
        )?)
    } else {
        None
    };

    let receipt = &drape_res.assessment.receipt;
    let gating = receipt.gating_flags.clone();
    let receipt_bytes = serde_json::to_vec_pretty(receipt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let surface_bytes = serde_json::to_vec_pretty(&drape_res.surface)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let surface_summary = json!({
        "file": "knit_surface.json",
        "sha256": sha256_hex(&surface_bytes),
        "pattern_sha256": drape_res.surface.pattern_sha256,
        "topology": drape_res.surface.topology,
        "vertices": drape_res.surface.vertices.len(),
        "triangles": drape_res.surface.triangles.len(),
        "boundary_loops": drape_res.surface.boundary_loops.len(),
        "course_growth_upward": drape_res.surface.course_growth_upward,
        "basis": "one vertex per source stitch; intrinsic charts and uncapped boundaries from actual parent-loop topology",
    });

    let mut files = vec![
        ("drape_uncertainty_receipt.json", receipt_bytes),
        ("knit_surface.json", surface_bytes),
        (
            "garment.obj",
            obj(&drape_res.solver.x, &drape_res.triangles, None).into_bytes(),
        ),
        (
            "body.obj",
            obj(&full.vertices, &full.triangles, None).into_bytes(),
        ),
    ];
    files.extend(selected_fabric.entries()?);
    if let Some(qualification) = &qual {
        files.push((
            "swatch_qualification.json",
            workspace::json_bytes(&json!(qualification))?,
        ));
    }
    if let Some(source) = source_bytes {
        files.push(("body_fit.json", source));
    }
    let patch_audit_val = if let Some(audit) = &patch_audit {
        let bytes = serde_json::to_vec_pretty(audit)
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
        files.push(("patch_audit.json", bytes));
        Some(
            serde_json::to_value(audit)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
        )
    } else {
        None
    };

    let mut report = json!({
        "schema": "ftx-knit-drape/1",
        "pattern": pattern_path,
        "pattern_schema": ftx_pattern::knit::KNIT_SCHEMA,
        "pattern_name": doc.name,
        "pattern_sha256": receipt.pattern_sha256,
        "material_sha256": receipt.material_sha256,
        "material": drape_res.material_report,
        "fabric_override": selected_fabric.receipt(),
        "swatch_qualification_sha256": qual.as_ref().map(KnitSwatchQualification::canonical_sha256).transpose()?,
        "swatch_qualification_scope": "optional gauge/process identity check; does not qualify constitutive response, physical garment performance, or fit",
        "measurements_sha256": sha256_hex(&meas_bytes),
        "body": description,
        "is_tubular": drape_res.is_tubular,
        "surface": surface_summary,
        "course_range": drape_res.course_range,
        "wale_range": drape_res.wale_range,
        "steps_run": drape_res.steps_run,
        "sew_steps_run": drape_res.sew_steps_run,
        "drape_steps_run": drape_res.drape_steps_run,
        "quiet_gravity_steps": drape_res.quiet_gravity_steps,
        "newton_iterations": drape_res.newton_iterations,
        "unconverged_steps": drape_res.unconverged_steps,
        "max_speed_m_s": drape_res.max_speed_m_s,
        "settled": drape_res.settled,
        "wall_s": (started.elapsed().as_secs_f64() * 10.0).round() / 10.0,
        "flags": gating,
        "gating": gating,
        "drape_uncertainty_receipt": receipt,
        "receipt_sha256": receipt.payload_sha256,
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    if let Some(audit) = &patch_audit_val {
        report["patch_audit"] = audit.clone();
    }
    write_drape_output(&out, &report, &files)?;

    let code = if gating.is_empty() && drape_res.settled {
        0
    } else {
        2
    };
    let mut file_names: Vec<&str> = std::iter::once("sim_report.json")
        .chain(files.iter().map(|(name, _)| *name))
        .collect();
    file_names.sort();
    file_names.dedup();
    let mut result = json!({
        "out": out,
        "schema": ftx_pattern::knit::KNIT_SCHEMA,
        "gating": gating,
        "settled": drape_res.settled,
        "is_tubular": drape_res.is_tubular,
        "surface": surface_summary,
        "steps_run": drape_res.steps_run,
        "sew_steps_run": drape_res.sew_steps_run,
        "drape_steps_run": drape_res.drape_steps_run,
        "quiet_gravity_steps": drape_res.quiet_gravity_steps,
        "material_sha256": receipt.material_sha256,
        "material": drape_res.material_report,
        "fabric_override": selected_fabric.receipt(),
        "drape_uncertainty_receipt": receipt,
        "receipt_sha256": receipt.payload_sha256,
        "files": file_names,
        "hardware_execution_authorized": false,
        "physical_knitting_validated": false,
        "fit_claimed": false,
    });
    if let Some(audit) = patch_audit_val {
        result["patch_audit"] = audit;
    }
    let text = format!(
        "knit drape '{}': {} gating flag(s), {} steps, jamming_margin={:.3}, max_strain_ratio={:.3}\nwrote {}",
        doc.name,
        gating.len(),
        drape_res.steps_run,
        receipt.jamming_margin,
        receipt.max_strain_ratio,
        out.display()
    );
    Ok((result, code, text))
}

fn cmd_knit_poincloux(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "knit poincloux",
        &[
            "mu",
            "dataset",
            "fit-min-strain",
            "fit-max-strain",
            "check-max-strain",
            "rmse-limit-n",
            "max-error-limit-n",
        ],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx knit poincloux [--mu MU] [--dataset published-sliding] [--json]",
    )?;
    if let Some(dataset) = args.options.get("dataset") {
        if dataset != "published-sliding" {
            return Err(bad_input("--dataset must be published-sliding"));
        }
        if args.options.contains_key("mu") {
            return Err(bad_input(
                "--mu belongs to the legacy surrogate; the published equation fits separate effective branch moduli",
            ));
        }
        use ftx_drape::knit::poincloux_measured::{ComparisonSettings, compare_measured_sliding};
        use ftx_pattern::material_catalog::poincloux::{BoundaryCondition, published_force_data};
        let data = published_force_data(BoundaryCondition::Sliding)?;
        let settings = ComparisonSettings {
            fit_strain_range: [
                opt_f64(args, "fit-min-strain", 0.02)?,
                opt_f64(args, "fit-max-strain", 0.15)?,
            ],
            check_max_strain: opt_f64(args, "check-max-strain", 1.0)?,
            max_rmse_n: opt_f64(args, "rmse-limit-n", 0.1)?,
            max_abs_error_n: opt_f64(args, "max-error-limit-n", 0.25)?,
        };
        let report = compare_measured_sliding(&data, settings)?;
        let code = if report.held_out_agreement { 0 } else { 2 };
        let text = format!(
            "Published sliding-swatch comparison: {} original measurements; FIT cycles 1-3, CHECK cycles 4-5; held-out criteria {}. Local production and wearer fit remain unqualified.",
            report.source_rows,
            if report.held_out_agreement {
                "passed"
            } else {
                "failed (inspect branch refusals and force residuals)"
            },
        );
        return Ok((
            serde_json::to_value(report)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?,
            code,
            text,
        ));
    }
    if [
        "fit-min-strain",
        "fit-max-strain",
        "check-max-strain",
        "rmse-limit-n",
        "max-error-limit-n",
    ]
    .iter()
    .any(|key| args.options.contains_key(*key))
    {
        return Err(bad_input(
            "measurement comparison options require --dataset published-sliding",
        ));
    }
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
        "model": "legacy_knit_elastica_surrogate",
        "independently_validated": false,
        "published_equation_6": false,
        "mu_friction": params.mu_friction,
        "points": rows,
        "comparison_command": "ftx knit poincloux --dataset published-sliding --json",
    });
    let text = format!(
        "Legacy knit surrogate (mu={:.2}): {} points, peak loading tension {:.2} N/m at strain 0.30. Use --dataset published-sliding for the published equation and measured force comparison.",
        params.mu_friction,
        curve.len(),
        curve.last().map_or(0.0, |p| p.tension_loading_n_m)
    );
    Ok((result, 0, text))
}

fn resolve_dye_machine(args: &Args) -> Result<DyeMachineProfile, Refusal> {
    let app_machine = match args.options.get("applicator").map(String::as_str) {
        None => None,
        Some("dip" | "dip-3bath" | "multi-bath-dip-v1") => Some("dip-3bath"),
        Some("piezo" | "piezo-cmyk" | "benchtop-piezo-cmyk-v1") => Some("piezo-cmyk"),
        Some(other) => {
            return Err(bad_input(format!(
                "unknown --applicator '{other}' (expected dip or piezo)"
            )));
        }
    };
    let explicit_machine = args.options.get("machine").map(String::as_str);
    let selected = match (app_machine, explicit_machine) {
        (Some(a), Some(m)) => {
            let pa = parse_dye_machine(a)?;
            let pm = parse_dye_machine(m)?;
            if pa.id != pm.id {
                return Err(bad_input(format!(
                    "--applicator '{a}' conflicts with --machine '{m}'"
                )));
            }
            return Ok(pm);
        }
        (Some(a), None) => a,
        (None, Some(m)) => m,
        (None, None) => "piezo-cmyk",
    };
    parse_dye_machine(selected)
}

fn read_dye_qualification_opt(args: &Args) -> Result<Option<YarnDyeingQualification>, Refusal> {
    let path = match (args.options.get("qual"), args.options.get("dye-qual")) {
        (Some(_), Some(_)) => {
            return Err(bad_input("supply either --qual or --dye-qual, not both"));
        }
        (Some(p), None) | (None, Some(p)) => p,
        (None, None) => return Ok(None),
    };
    let txt = read_utf8(&PathBuf::from(path), "yarn dyeing qualification")?;
    let q = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    Ok(Some(q))
}

/// Dispatches `ftx dye <swatch|plan|optimize|verify|export> ...`.
pub(super) fn cmd_dye(args: &Args) -> Result<(Value, i32, String), Refusal> {
    match args.positional.get(1).map(String::as_str) {
        Some("swatch") => cmd_dye_swatch(args),
        Some("plan") => cmd_dye_plan(args),
        Some("optimize") => cmd_dye_optimize(args),
        Some("verify") => cmd_dye_verify(args),
        Some("export") => cmd_dye_export(args),
        _ => Err(bad_input(
            "expected `ftx dye swatch|plan|optimize|verify|export`",
        )),
    }
}

fn cmd_dye_swatch(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "dye swatch",
        &["machine", "applicator", "yarn", "out"],
    )?;
    no_extra_positional(
        args,
        2,
        "ftx dye swatch --out NEW_DIR [--applicator dip|piezo] [--machine piezo-cmyk|dip-3bath] [--yarn ID] [--json]",
    )?;
    let machine = resolve_dye_machine(args)?;
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
        "fit_claimed": false,
    });
    let text = format!(
        "Generated Exp-4 dye bleed/fastness swatch kit for {} in {} ({} files; not_for_garment_wear=true)",
        machine.id,
        out_dir.display(),
        files.len()
    );
    Ok((result, 0, text))
}

fn knit_doc_to_stitch_map(
    doc: &KnitDoc,
    force_circular: bool,
    pore_open_fraction: f64,
) -> Result<(KnitStitchMap, i32), Refusal> {
    doc.validate()?;
    let mut c_min = u32::MAX;
    let mut c_max = u32::MIN;
    let mut w_min = i32::MAX;
    let mut w_max = i32::MIN;
    let mut front_cells = std::collections::BTreeSet::new();
    let mut has_tubular_overlap = false;
    for st in &doc.stitches {
        c_min = c_min.min(st.course);
        c_max = c_max.max(st.course);
        w_min = w_min.min(st.wale);
        w_max = w_max.max(st.wale);
        if matches!(st.bed, Bed::Front | Bed::FrontSlider) {
            front_cells.insert((st.course, st.wale));
        }
    }
    for st in &doc.stitches {
        if matches!(st.bed, Bed::Back | Bed::BackSlider)
            && front_cells.contains(&(st.course, st.wale))
        {
            has_tubular_overlap = true;
            break;
        }
    }
    let courses_raw = c_max.saturating_sub(c_min).saturating_add(1);
    let wales_i64 = i64::from(w_max) - i64::from(w_min) + 1;
    if wales_i64 <= 0
        || wales_i64 > 2_048
        || courses_raw > 4_096
        || (wales_i64 as u64).saturating_mul(u64::from(courses_raw)) > 4_000_000
    {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "knit dye stitch map {wales_i64} wales x {courses_raw} courses exceeds export preview budget (max 2048 wales, 4096 courses, 4000000 cells)"
            ),
        ));
    }
    let courses = courses_raw.max(1);
    let wales = (wales_i64 as u32).max(1);
    // Infer circular topology ONLY when both beds occupy the same `(course, wale)` (a real tube),
    // never on a flat panel with a 1x1 rib hem!
    let is_circular = force_circular || has_tubular_overlap;
    let default_loop_m = (2.6 * doc.gauge.blocked_wale_pitch_mm()) * 0.001;

    let mut ordered: Vec<&ftx_pattern::knit::StitchNode> = doc.stitches.iter().collect();
    ordered.sort_by_key(|s| {
        let is_back = matches!(s.bed, Bed::Back | Bed::BackSlider);
        // Flatbed alternates left-to-right / right-to-left every course; circular tubes traverse
        // front bed ascending then back bed descending around the cylinder (matching compile.rs).
        let rev = if is_circular {
            is_back
        } else {
            (s.course - c_min) % 2 == 1
        };
        let w_key = if rev {
            -i64::from(s.wale)
        } else {
            i64::from(s.wale)
        };
        (s.course, is_back, w_key, s.id)
    });

    let mut stitches = Vec::with_capacity(ordered.len());
    let mut s = 0.0_f64;
    let clamped_pore = pore_open_fraction.clamp(0.0, 0.95);
    for st in ordered {
        let len_m = st
            .yarn_length_mm
            .map_or(default_loop_m, |mm| (mm * 0.001).max(1e-4));
        let s0 = s;
        let s1 = s + len_m;
        s = s1;
        let wale_offset = (i64::from(st.wale) - i64::from(w_min)) as f64;
        let target_u = ((wale_offset + 0.5) / f64::from(wales)).clamp(0.0, 1.0);
        let target_v =
            (1.0 - (f64::from(st.course - c_min) + 0.5) / f64::from(courses)).clamp(0.0, 1.0);
        stitches.push(StitchArcMapping {
            course: st.course - c_min,
            wale: st.wale,
            s_start_m: s0,
            s_end_m: s1,
            target_u,
            target_v,
            pore_open_fraction: clamped_pore,
        });
    }
    let run_id = doc.yarn_runs.first().map_or(0, |r| r.run_id);
    Ok((
        KnitStitchMap {
            run_id,
            is_circular,
            courses,
            stitches_per_course: wales,
            total_yarn_length_m: s,
            stitches,
        },
        w_min,
    ))
}

fn render_dye_preview_png(
    stitch_map: &KnitStitchMap,
    w_min: i32,
    predicted_srgb: &[[u8; 3]],
) -> Vec<u8> {
    let w = stitch_map.stitches_per_course.max(1) as usize;
    let h = stitch_map.courses.max(1) as usize;
    let mut rgb = vec![220u8; w * h * 3];
    for (st, color) in stitch_map.stitches.iter().zip(predicted_srgb) {
        let col = (i64::from(st.wale) - i64::from(w_min)).clamp(0, w as i64 - 1) as usize;
        let row = h
            .saturating_sub(1)
            .saturating_sub((st.course as usize).min(h.saturating_sub(1)));
        let base = (row * w + col) * 3;
        rgb[base] = color[0];
        rgb[base + 1] = color[1];
        rgb[base + 2] = color[2];
    }
    ftx_drape::render::png(w, h, &rgb)
}

fn parse_drape_dye_context(
    path: &Path,
    expected_pattern_sha256: Option<&str>,
) -> Result<(f64, f64), Refusal> {
    let txt = read_utf8(path, "knit drape report or receipt")?;
    let val: Value = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let verify_receipt = |receipt: &DrapeUncertaintyReceipt| -> Result<(f64, f64), Refusal> {
        receipt.verify_seal()?;
        if let Some(expected) = expected_pattern_sha256
            && receipt.pattern_sha256 != expected
        {
            return Err(Refusal::new(
                RefusalCode::VerificationMismatch,
                format!(
                    "drape uncertainty receipt pattern_sha256 '{}' does not match supplied knit pattern '{expected}'",
                    receipt.pattern_sha256
                ),
            )
            .repair("pass the drape_uncertainty_receipt.json generated from this exact knit pattern"));
        }
        let stretch = receipt.max_strain_ratio.max(1.0);
        let pore = (0.05 + 0.35 * (stretch - 1.0)).clamp(0.0, 0.95);
        Ok((stretch, pore))
    };
    if val.get("schema").and_then(Value::as_str)
        == Some(ftx_pattern::knit::DRAPE_UNCERTAINTY_SCHEMA)
    {
        let receipt: DrapeUncertaintyReceipt = serde_json::from_value(val)
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
        return verify_receipt(&receipt);
    }
    if let Some(receipt_val) = val.get("drape_uncertainty_receipt")
        && receipt_val.is_object()
    {
        let receipt: DrapeUncertaintyReceipt = serde_json::from_value(receipt_val.clone())
            .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
        return verify_receipt(&receipt);
    }
    if let Some(stretch) = val
        .get("knit_shell")
        .and_then(|k| k.get("maximum_principal_stretch"))
        .and_then(Value::as_f64)
        .filter(|v| v.is_finite() && *v > 0.0)
    {
        let s = stretch.max(1.0);
        let pore = (0.05 + 0.35 * (s - 1.0)).clamp(0.0, 0.95);
        return Ok((s, pore));
    }
    Err(Refusal::new(
        RefusalCode::InvalidDocument,
        format!(
            "{}: --drape must be a sealed ftx-drape-uncertainty/1 receipt or knit drape sim_report.json",
            path.display()
        ),
    )
    .repair("pass drape_uncertainty_receipt.json or sim_report.json produced by `ftx drape`"))
}

fn cmd_dye_plan(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "dye plan",
        &[
            "machine",
            "applicator",
            "age-class",
            "regime",
            "drape",
            "image",
            "qual",
            "dye-qual",
            "courses",
            "wales",
            "circular",
            "drift-eps",
            "drift-sigma",
            "wick-mm",
            "max-iters",
            "cells",
            "name",
            "yarn",
            "out",
        ],
    )?;
    if args.positional.len() != 2 && args.positional.len() != 3 {
        return Err(bad_input(
            "usage: ftx dye plan [PATTERN.knit.json] --out NEW_DIR [--drape DRAPE.json] [--image PNG] [--qual Q.json] [--applicator dip|piezo] [--regime stripes|pooling|fiducial|inline|jacquard]",
        ));
    }
    let knit_doc: Option<KnitDoc> = match args.positional.get(2) {
        Some(path) => {
            let txt = read_utf8(Path::new(path), "knit pattern document")?;
            let doc: KnitDoc = serde_json::from_str(&txt)
                .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
            doc.validate()?;
            Some(doc)
        }
        None => None,
    };

    let machine = resolve_dye_machine(args)?;
    let wearer_age_class = match (args.options.get("age-class"), &knit_doc) {
        (Some(raw), Some(doc)) => {
            let parsed = parse_age_class(raw)?;
            if parsed != doc.wearer_age_class {
                return Err(Refusal::new(
                    RefusalCode::VerificationMismatch,
                    format!(
                        "--age-class {:?} conflicts with knit document wearer_age_class {:?}",
                        parsed, doc.wearer_age_class
                    ),
                )
                .repair("omit --age-class to inherit the knit document's wearer_age_class or match it exactly"));
            }
            parsed
        }
        (Some(raw), None) => parse_age_class(raw)?,
        (None, Some(doc)) => doc.wearer_age_class,
        (None, None) => WearerAgeClass::CalibrationSwatchOnly,
    };
    let regime = parse_regime(
        args.options
            .get("regime")
            .map(String::as_str)
            .unwrap_or("stripes"),
    )?;
    let qual = read_dye_qualification_opt(args)?;
    let knit_pattern_sha256 = knit_doc
        .as_ref()
        .map(KnitDoc::canonical_sha256)
        .transpose()?;
    let (aspect_ratio_stretch_bound, pore_open_fraction) = match args.options.get("drape") {
        Some(path) => parse_drape_dye_context(Path::new(path), knit_pattern_sha256.as_deref())?,
        None => (1.0, 0.05),
    };
    let drift = DriftParameters {
        systematic_eps: opt_f64(args, "drift-eps", 0.002)?,
        stochastic_sigma_eta: opt_f64(args, "drift-sigma", 0.015)?,
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
        let bytes = crate::read_regular_bounded(
            Path::new(img_path),
            MAX_KNIT_FILE_BYTES,
            "dye target image",
        )?;
        load_target_raster_with_limit(&bytes, 16_000_000)?
    } else {
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

    let (stitch_map, w_min) = if let Some(doc) = &knit_doc {
        knit_doc_to_stitch_map(
            doc,
            args.options.contains_key("circular"),
            pore_open_fraction,
        )?
    } else {
        let courses = opt_u32(args, "courses", 12)?;
        let wales = opt_u32(args, "wales", 12)?;
        if !(2..=256).contains(&courses) || !(2..=256).contains(&wales) {
            return Err(Refusal::new(
                RefusalCode::BudgetExceeded,
                format!(
                    "dye grid {courses}x{wales} must be within 2..=256 courses and 2..=256 wales"
                ),
            ));
        }
        (
            KnitStitchMap::regular_grid(
                0,
                courses,
                wales,
                0.010,
                args.options.contains_key("circular"),
                pore_open_fraction,
            ),
            0,
        )
    };

    let mut optics = DyeOpticsModel::synthetic_cmyk_cotton();
    optics.primaries.truncate(machine.primaries.len());
    let n_p = optics.primaries.len();
    let default_cells = stitch_map
        .courses
        .saturating_mul(stitch_map.stitches_per_course)
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
    let yarn_id = args
        .options
        .get("yarn")
        .cloned()
        .or_else(|| {
            knit_doc
                .as_ref()
                .and_then(|d| d.yarn_runs.first())
                .map(|r| r.yarn_id.clone())
        })
        .unwrap_or_else(|| "cotton-120tex".to_string());
    let inv_cfg = DyeInverseConfig {
        name: args
            .options
            .get("name")
            .cloned()
            .unwrap_or_else(|| "programmed_yarn_dye".to_string()),
        yarn_id,
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
    let uncertainty_ctx = DyeUncertaintyContext {
        qualification_sha256: qual
            .as_ref()
            .map(YarnDyeingQualification::canonical_sha256)
            .transpose()?,
        aspect_ratio_stretch_bound,
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
            .ok_or_else(|| bad_input("`ftx dye plan` requires --out NEW_DIR"))?,
    );
    let prog_json = serde_json::to_string_pretty(&opt.program)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let receipt_json = serde_json::to_string_pretty(&opt.uncertainty_receipt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let pareto_json = serde_json::to_string_pretty(&pareto)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let preview_png = render_dye_preview_png(&stitch_map, w_min, &opt.predicted_stitch_srgb);

    reserve_output(&out_dir, "dye plan")?;
    write_file(&out_dir, "dye_program.json", prog_json.as_bytes())?;
    write_file(
        &out_dir,
        "dye_uncertainty_receipt.json",
        receipt_json.as_bytes(),
    )?;
    write_file(&out_dir, "pareto_regimes.json", pareto_json.as_bytes())?;
    write_file(&out_dir, "simulated_preview.png", &preview_png)?;

    let files = vec![
        "dye_program.json",
        "dye_uncertainty_receipt.json",
        "pareto_regimes.json",
        "simulated_preview.png",
    ];
    let result = json!({
        "machine": machine.id,
        "regime": regime,
        "knit_pattern_sha256": knit_doc.as_ref().map(KnitDoc::canonical_sha256).transpose()?,
        "aspect_ratio_stretch_bound": aspect_ratio_stretch_bound,
        "delta_e00_p50": opt.delta_e00_p50,
        "delta_e00_p95": opt.delta_e00_p95,
        "gamut_floor_delta_e00": opt.gamut_floor_delta_e00,
        "unreproducible_energy_fraction": opt.unreproducible_energy_fraction,
        "coherent_horizon_circular": opt.coherent_horizons.0,
        "coherent_horizon_flatbed": opt.coherent_horizons.1,
        "uncertainty_receipt_sha256": opt.uncertainty_receipt.payload_sha256,
        "uncertainty_receipt": opt.uncertainty_receipt,
        "pareto_regimes": pareto,
        "files": files,
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
        "physical_dyeing_validated": false,
        "fit_claimed": false,
    });
    let text = format!(
        "Planned yarn dye program ({} stitches, {} cells, {} iters): p50 dE00={:.2}, p95 dE00={:.2}, horizon circ/flat={}/{} st -> {}",
        stitch_map.stitches.len(),
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
        &[
            "machine",
            "applicator",
            "qual",
            "dye-qual",
            "receipt",
            "material-sha256",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        3,
        "ftx dye verify PROGRAM.json [--applicator dip|piezo] [--machine ID] [--qual Q.json] [--receipt RECEIPT.json] [--material-sha256 SHA] [--out NEW_DIR] [--json]",
    )?;
    let prog_path = PathBuf::from(&args.positional[2]);
    let txt = read_utf8(&prog_path, "dye program document")?;
    let prog: YarnDyeProgram = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let machine = resolve_dye_machine(args)?;
    let qual = read_dye_qualification_opt(args)?;
    let expected_material_sha256 = expected_knit_material(args)?;
    let receipt: Option<DyePlanUncertaintyReceipt> =
        read_optional_knit_json(args, "receipt", "dye plan uncertainty receipt")?;
    let bundle = verify_and_export_dye_program_with_receipt(
        &prog,
        &machine,
        qual.as_ref(),
        expected_material_sha256,
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
        "hardware_execution_authorized": false,
        "physical_dyeing_validated": false,
        "fit_claimed": false,
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

fn cmd_dye_export(args: &Args) -> Result<(Value, i32, String), Refusal> {
    known_options(
        args,
        "dye export",
        &[
            "machine",
            "applicator",
            "qual",
            "dye-qual",
            "receipt",
            "material-sha256",
            "out",
        ],
    )?;
    no_extra_positional(
        args,
        3,
        "ftx dye export PROGRAM.json --out NEW_DIR [--qual Q.json] [--receipt RECEIPT.json] [--applicator dip|piezo] [--machine ID] [--material-sha256 SHA] [--json]",
    )?;
    let out_dir = PathBuf::from(
        args.options
            .get("out")
            .ok_or_else(|| bad_input("`ftx dye export` requires --out NEW_DIR"))?,
    );
    let prog_path = PathBuf::from(&args.positional[2]);
    let txt = read_utf8(&prog_path, "dye program document")?;
    let prog: YarnDyeProgram = serde_json::from_str(&txt)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let machine = resolve_dye_machine(args)?;
    let qual = read_dye_qualification_opt(args)?;
    let expected_material_sha256 = expected_knit_material(args)?;
    let receipt: Option<DyePlanUncertaintyReceipt> =
        read_optional_knit_json(args, "receipt", "dye plan uncertainty receipt")?;
    let bundle = verify_and_export_dye_program_with_receipt(
        &prog,
        &machine,
        qual.as_ref(),
        expected_material_sha256,
        receipt.as_ref(),
    )?;

    reserve_output(&out_dir, "dye export")?;
    write_file(&out_dir, "dye_schedule.csv", bundle.schedule_csv.as_bytes())?;
    write_file(
        &out_dir,
        "dye_controller.gcode",
        bundle.controller_gcode.as_bytes(),
    )?;
    write_file(&out_dir, "dye_cert.json", bundle.cert_json.as_bytes())?;

    let files = vec!["dye_schedule.csv", "dye_controller.gcode", "dye_cert.json"];
    let result = json!({
        "verified": true,
        "machine": machine.id,
        "gcode_sha256": bundle.gcode_sha256,
        "cert": bundle.cert,
        "files": files,
        "out_dir": out_dir.display().to_string(),
        "hardware_execution_authorized": false,
        "physical_dyeing_validated": false,
        "fit_claimed": false,
    });
    let text = format!(
        "Exported dye controller for '{}' ({}): peak wet tension {:.2} N, dwell [{:.2}, {:.2}] s -> {} (hardware_execution_authorized=false)",
        prog.name,
        machine.id,
        bundle.cert.hazard_report.peak_wet_tension_n,
        bundle.cert.hazard_report.min_oven_dwell_s,
        bundle.cert.hazard_report.max_oven_dwell_s,
        out_dir.display()
    );
    Ok((result, 0, text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;
    use ftx_pattern::knit::draft_shaped_panel;
    use ftx_pattern::materials::Evidence;

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
    fn swatch_defaults_export_without_rounding_explicit_cams() {
        let root = fresh_dir("default-cams");
        for (machine, expected) in [
            ("kniterate-7e", [5.0, 8.0]),
            ("shima-12e", [34.7, 54.5]),
            ("stoll-12e", [7.0, 11.0]),
        ] {
            let out = root.join(machine);
            let args = parse(
                &[
                    "knit",
                    "swatch",
                    "--machine",
                    machine,
                    "--needles",
                    "20",
                    "--courses-per-zone",
                    "8",
                    "--created",
                    "2026-11-27T12:00",
                    "--out",
                    out.to_str().unwrap(),
                ]
                .map(String::from),
            )
            .unwrap();
            let (result, code, _) = cmd_knit(&args).unwrap();
            assert_eq!(code, 0);
            let doc: KnitDoc =
                serde_json::from_slice(&std::fs::read(out.join("swatch.knit.json")).unwrap())
                    .unwrap();
            assert!(
                doc.stitches
                    .iter()
                    .all(|stitch| expected.contains(&stitch.cam_stitch_setting.setting))
            );
            for cam in expected {
                assert!(
                    doc.stitches
                        .iter()
                        .any(|stitch| stitch.cam_stitch_setting.setting == cam)
                );
            }
            if machine == "kniterate-7e" {
                assert!(out.join("swatch.kc").is_file());
                assert!(result["backend_export_refusal"].is_null());
                assert!(result["kcode_verification"].is_object());
            }
        }

        let rejected_out = root.join("explicit-quarter-cam");
        let args = parse(
            &[
                "knit",
                "swatch",
                "--cams",
                "5.25,8.25",
                "--created",
                "2026-11-27T12:00",
                "--out",
                rejected_out.to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        let refusal = cmd_knit(&args).unwrap_err();
        assert!(refusal.message.contains("integer tenths"));
        assert!(!rejected_out.exists());
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

        // Generic 12E V-bed and slider-bed profiles resolve via CLI `--machine`.
        for machine_id in ["generic-vbed-12e", "generic-slider-12e"] {
            let vbed_dir = root.join(format!("swatch-{machine_id}"));
            let vbed_args = parse(
                &[
                    "knit",
                    "swatch",
                    "--machine",
                    machine_id,
                    "--needles",
                    "16",
                    "--courses-per-zone",
                    "6",
                    "--created",
                    "2026-11-27T12:00",
                    "--out",
                    vbed_dir.to_str().unwrap(),
                ]
                .map(String::from),
            )
            .unwrap();
            let (vbed_val, vbed_code, _) = cmd_knit(&vbed_args).unwrap();
            assert_eq!(vbed_code, 0);
            assert_eq!(vbed_val["machine"], machine_id);
            assert!(vbed_dir.join("swatch.k").is_file());
        }

        // `ftx dye plan` refuses unrecognized `--drape` JSON, refuses `--age-class` downgrades of
        // an infant knit doc, and refuses huge wale-span knit docs before preview allocation.
        let bogus_drape = root.join("bogus_drape.json");
        std::fs::write(&bogus_drape, br#"{"unrelated": true}"#).unwrap();
        let bad_drape_plan = parse(
            &[
                "dye",
                "plan",
                "--drape",
                bogus_drape.to_str().unwrap(),
                "--out",
                root.join("never-bad-drape").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_dye(&bad_drape_plan).unwrap_err().code,
            RefusalCode::InvalidDocument
        );
        assert!(!root.join("never-bad-drape").exists());

        let infant_downgrade = parse(
            &[
                "dye",
                "plan",
                hand_dir.join("pattern.knit.json").to_str().unwrap(),
                "--age-class",
                "adult",
                "--out",
                root.join("never-downgrade").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_dye(&infant_downgrade).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        assert!(!root.join("never-downgrade").exists());

        let mut huge_knit: KnitDoc =
            serde_json::from_slice(&std::fs::read(swatch_dir.join("swatch.knit.json")).unwrap())
                .unwrap();
        huge_knit.stitches[0].wale = -500_000;
        huge_knit.stitches[1].wale = 500_000;
        let huge_knit_path = root.join("huge_span.knit.json");
        std::fs::write(&huge_knit_path, serde_json::to_vec(&huge_knit).unwrap()).unwrap();
        let huge_plan = parse(
            &[
                "dye",
                "plan",
                huge_knit_path.to_str().unwrap(),
                "--out",
                root.join("never-huge").to_str().unwrap(),
            ]
            .map(String::from),
        )
        .unwrap();
        assert_eq!(
            cmd_dye(&huge_plan).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );
        assert!(!root.join("never-huge").exists());
    }

    #[test]
    fn published_force_comparison_reaches_cli_and_keeps_failed_physical_checks() {
        let args =
            parse(&["knit", "poincloux", "--dataset", "published-sliding"].map(String::from))
                .unwrap();
        let (result, code, text) = cmd_knit(&args).unwrap();
        assert_eq!(code, 2);
        assert_eq!(result["source_rows"], 18_141);
        assert_eq!(result["fit_cycles"], json!([1, 2, 3]));
        assert_eq!(result["check_cycles"], json!([4, 5]));
        assert_eq!(result["held_out_agreement"], false);
        assert!(result["branches"][1]["fit_refusal"].is_object());
        assert_eq!(result["local_production_qualified"], false);
        assert_eq!(result["wearer_fit_claimed"], false);
        assert!(text.contains("criteria failed"));
        let mixed = parse(
            &[
                "knit",
                "poincloux",
                "--dataset",
                "published-sliding",
                "--mu",
                "0.2",
            ]
            .map(String::from),
        )
        .unwrap();
        assert!(cmd_knit(&mixed).is_err());
        let legacy = parse(&["knit", "poincloux"].map(String::from)).unwrap();
        let (result, code, _) = cmd_knit(&legacy).unwrap();
        assert_eq!(code, 0);
        assert_eq!(result["independently_validated"], false);
        assert_eq!(result["published_equation_6"], false);
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
        assert!(result["backend_export_refusal"].is_null());
        assert!(out_dir.join("program.kc").exists());
        assert!(out_dir.join("kcode_verification.json").exists());
        assert_eq!(
            result["kcode_verification"]["hardware_execution_authorized"],
            false
        );
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

        let cables_path = root.join("cables.json");
        std::fs::write(
            &cables_path,
            serde_json::to_vec(&json!([{
                "course": 4, "left_wales": [-2, -1], "right_wales": [0, 1], "left_in_front": true
            }]))
            .unwrap(),
        )
        .unwrap();
        let constructions: &[(&str, &[(&str, &str)])] = &[
            (
                "neckline",
                &[
                    ("neck-mm", "40"),
                    ("neck-depth-mm", "16"),
                    ("shoulder-short-row-pairs", "1"),
                ],
            ),
            (
                "ribbed-tube",
                &[
                    ("style", "tube"),
                    ("chest-mm", "120"),
                    ("neck-mm", "80"),
                    ("yoke-mm", "48"),
                    ("length-mm", "48"),
                    ("rib-courses", "2"),
                    ("collar-rib", "2"),
                ],
            ),
            (
                "branched",
                &[
                    ("style", "pullover"),
                    ("chest-mm", "120"),
                    ("neck-mm", "80"),
                    ("yoke-mm", "48"),
                    ("length-mm", "48"),
                    ("sleeve-cuff-mm", "40"),
                    ("upper-arm-mm", "60"),
                    ("sleeve-length-mm", "48"),
                    ("cuff-rib", "2"),
                    ("rib-courses", "2"),
                    ("collar-rib", "2"),
                ],
            ),
            ("cabled", &[("cables", cables_path.to_str().unwrap())]),
        ];
        for (label, options) in constructions {
            let mut request = Args {
                positional: args.positional.clone(),
                options: args.options.clone(),
                values: args.values.clone(),
                json: args.json,
            };
            let destination = root.join(label);
            request
                .options
                .insert("out".into(), destination.to_str().unwrap().into());
            for (key, value) in *options {
                request.options.insert((*key).into(), (*value).into());
            }
            let (output, exit, _) =
                cmd_knit(&request).unwrap_or_else(|error| panic!("{label}: {error:?}"));
            assert_eq!(exit, 0, "{label}");
            assert!(
                output["backend_export_refusal"].is_null(),
                "{label}: {}",
                output["backend_export_refusal"]
            );
            let saved: KnitDoc = serde_json::from_slice(
                &std::fs::read(destination.join("pattern.knit.json")).unwrap(),
            )
            .unwrap();
            let compiled = compile_knit_doc(
                &saved,
                &profile,
                Some(&serde_json::from_value(qualification.clone()).unwrap()),
                Some(&material_sha256),
                None,
            )
            .unwrap();
            let kc = std::fs::read_to_string(destination.join("program.kc")).unwrap();
            ftx_export::knit::verify_kniterate_kc_roundtrip(&kc, &compiled, &profile).unwrap();
            let budget = &output["kcode_verification"]["takedown"];
            assert_eq!(budget["physical_takedown_calibrated"], false);
            assert!(
                budget["max_held_advance_units"].as_u64().unwrap()
                    <= budget["limit_units"].as_u64().unwrap()
            );
            match *label {
                "neckline" => assert_eq!(saved.short_row_turns.len(), 2),
                "branched" => assert_eq!(
                    saved.metadata["boundary_rings"],
                    "waist_hem,neck,left_wrist_cuff,right_wrist_cuff"
                ),
                "cabled" => assert_eq!(saved.cable_groups.len(), 1),
                _ => assert_eq!(saved.metadata["tube_structure"], "half_gauge_1x1_rib"),
            }
        }

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

    #[test]
    fn dye_stitch_map_preserves_flat_rib_parity_and_drape_receipt_binding() {
        let profile = MachineProfile::kniterate_7e();
        let gauge = GaugeSpec {
            wales_per_100mm: 22.0,
            courses_per_100mm: 30.0,
            needle_pitch_mm: profile.needle_pitch_mm,
            yarn_tex: Some(200.0),
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        };
        let panel = draft_shaped_panel(
            &ParametricPanelSpec {
                name: "ribbed_flat_panel".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                hem_width_mm: 200.0,
                chest_width_mm: 220.0,
                length_mm: 200.0,
                armhole_depth_mm: 60.0,
                armhole_inset_total_mm: 24.0,
                neck_width_mm: 0.0,
                neck_depth_mm: 0.0,
                rib_hem_courses: 4,
                cam_setting: 7.0,
                yarn_id: "merino-200tex".to_string(),
                carrier_id: ftx_pattern::knit::CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &profile,
        )
        .unwrap();
        let (panel_map, _) = knit_doc_to_stitch_map(&panel, false, 0.05).unwrap();
        assert!(
            !panel_map.is_circular,
            "flat panel with 1x1 rib hem must stay flatbed boustrophedon"
        );
        let c1_wales: Vec<i32> = panel_map
            .stitches
            .iter()
            .filter(|s| s.course == 1)
            .map(|s| s.wale)
            .collect();
        assert!(
            c1_wales.windows(2).all(|w| w[0] >= w[1]),
            "odd flatbed course must traverse right-to-left: {c1_wales:?}"
        );

        let tube = draft_seamless_raglan_tube(
            &RaglanPulloverSpec {
                name: "circular_tube".to_string(),
                wearer_age_class: WearerAgeClass::Child3To14,
                neck_girth_mm: 240.0,
                chest_girth_mm: 400.0,
                yoke_depth_mm: 60.0,
                body_length_below_arm_mm: 80.0,
                collar_rib_courses: 0,
                hem_rib_courses: 0,
                cam_setting: 7.0,
                yarn_id: "merino-200tex".to_string(),
                carrier_id: ftx_pattern::knit::CarrierId("1".to_string()),
                has_hood_or_neck_drawstring: false,
            },
            &gauge,
            &profile,
        )
        .unwrap();
        let (tube_map, _) = knit_doc_to_stitch_map(&tube, false, 0.05).unwrap();
        assert!(tube_map.is_circular);
        let c0: Vec<i32> = tube_map
            .stitches
            .iter()
            .filter(|s| s.course == 0)
            .map(|s| s.wale)
            .collect();
        let half = c0.len() / 2;
        assert!(
            c0[..half].windows(2).all(|w| w[0] <= w[1]),
            "front half of circular tube must ascend"
        );
        assert!(
            c0[half..].windows(2).all(|w| w[0] >= w[1]),
            "back half of circular tube must descend seamlessly around the cylinder"
        );

        // Sealed drape receipt from another pattern must refuse with VerificationMismatch.
        let root = fresh_dir("drape-bind");
        let receipt = DrapeUncertaintyReceipt::seal(
            panel.canonical_sha256().unwrap(),
            "mat-sha".to_string(),
            1.15,
            0.25,
            0.0,
            0.0,
            Vec::new(),
            Evidence::Estimate,
        )
        .unwrap();
        let receipt_path = root.join("receipt.json");
        std::fs::write(&receipt_path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        assert!(
            parse_drape_dye_context(&receipt_path, Some(&panel.canonical_sha256().unwrap()))
                .is_ok()
        );
        assert_eq!(
            parse_drape_dye_context(&receipt_path, Some(&tube.canonical_sha256().unwrap()))
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
    }
}
