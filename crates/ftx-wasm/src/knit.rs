//! Browser & portable knitting + programmed continuous yarn dyeing bindings (`FTX-K18`).
//!
//! All functions accept and return bounded deterministic UTF-8 JSON (strictly respecting native
//! refusal codes, `hardware_execution_authorized: false`, and zero browser-type leakage into
//! numerical crates).

use ftx_core::{Refusal, RefusalCode, TargetRaster};
use ftx_drape::dye::{
    CapillaryTransportConfig, DriftParameters, DyeInverseConfig, DyeOpticsModel, KnitStitchMap,
    optimize_yarn_dye_program, rank_five_regime_pareto, srgb_u8_to_xyz_d65,
};
use ftx_export::dye::verify_and_export_dye_program;
use ftx_export::knit::{
    check_hand_knit_svg, export_brother_ayab, export_calibration_swatch_kit,
    export_hand_knit_document, export_kniterate_kc,
};
use ftx_pattern::dye::{DyeMachineProfile, DyeingRegime};
use ftx_pattern::knit::{
    CarrierId, GaugeSpec, MachineBackendKind, MachineProfile, ParametricPanelSpec, WearerAgeClass,
    compile_knit_doc, draft_shaped_panel,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

const MAX_INPUT_BYTES: usize = 4 * 1024 * 1024;
const MAX_OUTPUT_BYTES: usize = 16 * 1024 * 1024;
const SWATCH_MATERIAL_PLACEHOLDER: &str = "browser-unqualified-yarn-placeholder";

/// Request payload for [`knit_swatch_json`].
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitSwatchRequest {
    /// Target machine ID (`"kniterate-7e"`, `"brother-kh930-ayab"`, `"brother-kh270-ayab"`, `"shima-seiki-12e-handoff"`, `"hand-knit"`).
    pub machine: String,
    /// Yarn identifier label.
    pub yarn_id: String,
    /// Active needle width (`4..=80`).
    pub needles: u32,
    /// Courses per stepped cam zone.
    pub courses_per_zone: u32,
    /// Dimensionless cam stitch settings to step across.
    pub cam_settings: Vec<f64>,
    /// Deterministic creation date (`YYYY-MM-DDTHH:MM`).
    #[serde(default = "default_created")]
    pub created: String,
}

/// Request payload for [`knit_panel_preview_json`].
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnitPanelPreviewRequest {
    /// Target machine ID.
    pub machine: String,
    /// Panel specification (`ftx-knit/1` measured drafting inputs).
    pub panel: ParametricPanelSpec,
    /// Gauge specification.
    pub gauge: GaugeSpec,
    /// Deterministic creation date (`YYYY-MM-DDTHH:MM`).
    #[serde(default = "default_created")]
    pub created: String,
}

/// Request payload for [`dye_optimize_json`].
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DyeOptimizeRequest {
    /// Dye machine preset (`"piezo-cmyk"` or `"dip-3bath"`).
    pub machine: String,
    /// Required wearer age class (non-defaulting).
    pub wearer_age_class: WearerAgeClass,
    /// Target dyeing regime.
    pub regime: DyeingRegime,
    /// Target raster width (`1..=64` in interactive browser mode).
    pub width: u32,
    /// Target raster height (`1..=64` in interactive browser mode).
    pub height: u32,
    /// Straight sRGB pixels (`width * height`).
    pub srgb_u8: Vec<[u8; 3]>,
    /// Optional alpha weights (`0..=1`, defaults to all `1.0`).
    #[serde(default)]
    pub alpha_weight: Vec<f32>,
    /// Knitted grid dimensions `[courses, wales]`.
    pub grid: [u32; 2],
    /// True for circular tubular knitting; false for flatbed boustrophedon.
    pub is_circular: bool,
    /// Systematic drift `epsilon`.
    pub drift_eps: f64,
    /// Stochastic Brownian per-stitch drift `sigma_eta`.
    pub drift_sigma_eta: f64,
    /// Capillary wicking half-width in mm.
    pub wick_mm: f64,
    /// Maximum optimizer iterations (`1..=40`).
    #[serde(default = "default_iters")]
    pub max_iters: usize,
}

/// Structured browser output envelope.
#[derive(Clone, Debug, Serialize)]
pub struct KnitWasmEnvelope {
    /// True on success.
    pub ok: bool,
    /// Result payload when `ok == true`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    /// Structured refusal when `ok == false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refusal: Option<Refusal>,
}

fn default_created() -> String {
    "2026-11-27T08:00".to_string()
}

const fn default_iters() -> usize {
    20
}

/// Resolves a named built-in knitting [`MachineProfile`].
///
/// # Errors
/// [`RefusalCode::UnknownParameter`] for unrecognized machine IDs.
pub fn resolve_knit_machine(id: &str) -> Result<MachineProfile, Refusal> {
    match id {
        "kniterate-7e" | "kniterate" => Ok(MachineProfile::kniterate_7e()),
        "brother-kh930-ayab" | "brother-kh930" | "kh930" => Ok(MachineProfile::brother_kh930_ayab()),
        "brother-kh270-ayab" | "brother-kh270" | "kh270" => Ok(MachineProfile::brother_kh270_ayab()),
        "shima-seiki-12e-handoff" | "shima-12e" | "shima" => {
            Ok(MachineProfile::shima_seiki_12e_handoff())
        }
        "hand-knit" | "hand" => Ok(MachineProfile::hand_knit(4.0)),
        _ => Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown knitting machine profile '{id}'"),
        )
        .repair("choose one of: kniterate-7e, brother-kh930-ayab, brother-kh270-ayab, shima-seiki-12e-handoff, hand-knit")),
    }
}

/// Resolves a named built-in [`DyeMachineProfile`].
///
/// # Errors
/// [`RefusalCode::UnknownParameter`] for unrecognized dye machine IDs.
pub fn resolve_dye_machine(id: &str) -> Result<DyeMachineProfile, Refusal> {
    match id {
        "piezo-cmyk" | "benchtop-piezo-cmyk-v1" => Ok(DyeMachineProfile::benchtop_piezo_cmyk()),
        "dip-3bath" | "multi-bath-dip-v1" => Ok(DyeMachineProfile::multi_bath_dip_rig()),
        _ => Err(Refusal::new(
            RefusalCode::UnknownParameter,
            format!("unknown dye machine profile '{id}'"),
        )
        .repair("choose one of: piezo-cmyk, dip-3bath")),
    }
}

/// Generates a verified `CalibrationSwatchOnly` kit (Knitout v2, target backend file, vector CYC
/// SVG chart, sealed `ftx-machine-cert/1`, and blank swatch qualification template) as JSON.
///
/// # Errors
/// Budget, JSON, or safety-cap refusals.
pub fn knit_swatch_json(request_json: &str) -> Result<String, Refusal> {
    if request_json.len() > MAX_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "knit swatch request exceeds 4 MiB budget",
        ));
    }
    let req: KnitSwatchRequest = serde_json::from_str(request_json)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let profile = resolve_knit_machine(&req.machine)?;
    let kit = export_calibration_swatch_kit(
        &profile,
        &req.yarn_id,
        SWATCH_MATERIAL_PLACEHOLDER,
        req.needles,
        req.courses_per_zone,
        &req.cam_settings,
    )?;
    let compiled = compile_knit_doc(
        &kit.doc,
        &profile,
        None,
        Some(SWATCH_MATERIAL_PLACEHOLDER),
        None,
    )?;
    if compiled.cert.cert_sha256 != kit.knitout.cert.cert_sha256 {
        return Err(Refusal::new(
            RefusalCode::VerificationMismatch,
            "calibration swatch kit certificate digest disagreed with compiled bundle",
        ));
    }
    let hand = export_hand_knit_document(&kit.doc, &req.created)?;
    let svg_checks = check_hand_knit_svg(&hand.svg_text, "swatch.svg");
    let kc_text = if profile.backend_kind == MachineBackendKind::KniterateKc {
        Some(export_kniterate_kc(&compiled, &profile)?.kc_text)
    } else {
        None
    };
    let ayab = if profile.backend_kind == MachineBackendKind::BrotherAyab {
        Some(export_brother_ayab(&kit.doc, &profile, &compiled.cert)?)
    } else {
        None
    };

    let payload = json!({
        "machine": profile,
        "knitout_text": kit.knitout.knitout_text,
        "knitout_sha256": kit.knitout.knitout_sha256,
        "kc_text": kc_text,
        "ayab_checklist": ayab.as_ref().map(|b| &b.checklist),
        "ayab_png_base64": ayab.as_ref().map(|b| encode_base64(&b.png_bytes)),
        "ayab_png_sha256": ayab.as_ref().map(|b| &b.png_sha256),
        "chart_svg": hand.svg_text,
        "instructions_text": hand.instructions_text,
        "svg_checks_passed": svg_checks.iter().all(|c| c.ok),
        "cert": kit.knitout.cert,
        "qualification_template": kit.qualification_template,
    });
    encode_bounded_json(&payload)
}

/// Drafts a measured fully-fashioned panel and exports its pure-vector CYC SVG chart & written guide
/// (plus child-safety validation) as bounded JSON.
///
/// # Errors
/// Budget, JSON, or child-safety/geometry refusals.
pub fn knit_panel_preview_json(request_json: &str) -> Result<String, Refusal> {
    if request_json.len() > MAX_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "knit panel request exceeds 4 MiB budget",
        ));
    }
    let req: KnitPanelPreviewRequest = serde_json::from_str(request_json)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    let profile = resolve_knit_machine(&req.machine)?;
    let doc = draft_shaped_panel(&req.panel, &req.gauge, &profile)?;
    let hand = export_hand_knit_document(&doc, &req.created)?;
    let svg_checks = check_hand_knit_svg(&hand.svg_text, "panel.svg");
    let payload = json!({
        "doc_sha256": doc.canonical_sha256()?,
        "stitch_count": doc.stitches.len(),
        "short_row_turns": doc.short_row_turns.len(),
        "metadata": doc.metadata,
        "chart_svg": hand.svg_text,
        "instructions_text": hand.instructions_text,
        "svg_checks_passed": svg_checks.iter().all(|c| c.ok),
        "fit_claimed": false,
        "hardware_execution_authorized": false,
    });
    encode_bounded_json(&payload)
}

/// Runs the 4-stage radiometric Kubelka–Munk + Sobol-CVaR Bertsekas L-BFGS continuous yarn dyeing
/// optimizer and 5-regime Pareto ranker in-browser, returning predicted stitch colors, CIEDE2000
/// percentiles, coherent horizons, and (for swatch mode) verified `.csv`/`.gcode`/cert!
///
/// # Errors
/// Budget, JSON, or optimization/hazard refusals.
pub fn dye_optimize_json(request_json: &str) -> Result<String, Refusal> {
    if request_json.len() > MAX_INPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "dye optimize request exceeds 4 MiB budget",
        ));
    }
    let req: DyeOptimizeRequest = serde_json::from_str(request_json)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    if req.width == 0 || req.height == 0 || req.width > 64 || req.height > 64 {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "interactive browser dye target raster must be within 1..=64 x 1..=64 pixels",
        ));
    }
    let [courses, wales] = req.grid;
    if !(2..=40).contains(&courses) || !(2..=40).contains(&wales) {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            format!(
                "interactive browser dye grid [{courses}, {wales}] must be within 2..=40 x 2..=40"
            ),
        ));
    }
    if !(1..=40).contains(&req.max_iters) {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            format!(
                "interactive browser dye max_iters {} must be within 1..=40",
                req.max_iters
            ),
        ));
    }
    Refusal::finite("drift_eps", req.drift_eps)?;
    Refusal::finite("drift_sigma_eta", req.drift_sigma_eta)?;
    Refusal::finite("wick_mm", req.wick_mm)?;
    if req.drift_sigma_eta < 0.0 || req.wick_mm < 0.0 {
        return Err(Refusal::new(
            RefusalCode::OutOfRange,
            "drift_sigma_eta and wick_mm cannot be negative",
        ));
    }
    let machine = resolve_dye_machine(&req.machine)?;
    let target = if req.alpha_weight.is_empty() {
        TargetRaster::from_opaque_srgb(req.width, req.height, req.srgb_u8)?
    } else {
        let mut h = ftx_core::receipt::Sha256::new();
        h.update(&req.width.to_le_bytes());
        h.update(&req.height.to_le_bytes());
        for px in &req.srgb_u8 {
            h.update(px);
        }
        for &a in &req.alpha_weight {
            let norm = if a == 0.0 { 0.0_f32 } else { a };
            h.update(&norm.to_le_bytes());
        }
        TargetRaster::new(
            req.width,
            req.height,
            req.srgb_u8,
            req.alpha_weight,
            h.finalize(),
        )?
    };

    let stitch_map = KnitStitchMap::regular_grid(0, courses, wales, 0.010, req.is_circular, 0.06);
    let optics = DyeOpticsModel::synthetic_cmyk_cotton();
    // Match optics primary count to machine primary count
    let mut optics_matched = optics.clone();
    optics_matched.primaries.truncate(machine.primaries.len());
    let n_p = optics_matched.primaries.len();
    let n_cells = ((courses * wales) as usize / 2).clamp(8, 128);
    let tr_cfg = CapillaryTransportConfig {
        ds_m: stitch_map.total_yarn_length_m / (n_cells as f64),
        sigma_wick_m: req.wick_mm * 0.001,
        rf_retardations: machine
            .primaries
            .iter()
            .take(n_p)
            .map(|p| p.rf_retardation)
            .collect(),
        fixation_efficiency: vec![0.94; n_p],
        shrinkage_axial_frac: 0.02,
    };
    let drift = DriftParameters {
        systematic_eps: req.drift_eps,
        stochastic_sigma_eta: req.drift_sigma_eta,
    };
    let inv_cfg = DyeInverseConfig {
        name: "browser-dye-program".to_string(),
        yarn_id: "cotton-120tex".to_string(),
        wearer_age_class: req.wearer_age_class,
        regime: req.regime,
        n_cells,
        n_sobol_scenarios: 8,
        cvar_alpha: 0.90,
        cvar_tau: 0.5,
        lambda_smooth: 0.05,
        lambda_switch: 0.02,
        max_iters: req.max_iters,
        skin_underlayer_xyz: srgb_u8_to_xyz_d65([215, 180, 155]),
        ciede_eps_c: 1e-4,
        drift,
    };
    let opt = optimize_yarn_dye_program(
        &target,
        &stitch_map,
        &machine,
        &optics_matched,
        &tr_cfg,
        &inv_cfg,
    )?;
    let pareto = rank_five_regime_pareto(&target, &stitch_map, &machine, drift, 4);

    // Emit controller G-code/CSV automatically for CalibrationSwatchOnly; wearable garments require Exp-4 qual!
    // Surface any physical hazard refusal explicitly rather than swallowing it behind `.ok()`.
    let (verified_export, controller_export_refusal) =
        if req.wearer_age_class == WearerAgeClass::CalibrationSwatchOnly {
            match verify_and_export_dye_program(&opt.program, &machine, None, None) {
                Ok(bundle) => (Some(bundle), None),
                Err(refusal) => (None, Some(refusal)),
            }
        } else {
            (None, None)
        };

    // Re-index yarn-path stitch predictions into canonical `(course, wale)` row-major grid order
    // so flatbed boustrophedon odd courses render at their true physical wale positions.
    let grid_courses = courses as usize;
    let grid_wales = wales as usize;
    let mut predicted_grid = vec![[220_u8, 220, 220]; grid_courses * grid_wales];
    for (st, &rgb) in stitch_map.stitches.iter().zip(&opt.predicted_stitch_srgb) {
        if let Ok(wale) = usize::try_from(st.wale) {
            let row = st.course as usize;
            if row < grid_courses && wale < grid_wales {
                predicted_grid[row * grid_wales + wale] = rgb;
            }
        }
    }

    let payload = json!({
        "delta_e00_p50": opt.delta_e00_p50,
        "delta_e00_p95": opt.delta_e00_p95,
        "gamut_floor_delta_e00": opt.gamut_floor_delta_e00,
        "unreproducible_energy_fraction": opt.unreproducible_energy_fraction,
        "coherent_horizon_circular_stitches": opt.coherent_horizons.0,
        "coherent_horizon_flatbed_stitches": opt.coherent_horizons.1,
        "iterations": opt.iterations,
        "grid": [courses, wales],
        "predicted_stitch_srgb": predicted_grid,
        "pareto_regimes": pareto,
        "program": opt.program,
        "schedule_csv": verified_export.as_ref().map(|b| &b.schedule_csv),
        "controller_gcode": verified_export.as_ref().map(|b| &b.controller_gcode),
        "cert": verified_export.as_ref().map(|b| &b.cert),
        "controller_export_refusal": controller_export_refusal,
        "hardware_execution_authorized": false,
        "physical_dyeing_validated": false,
    });
    encode_bounded_json(&payload)
}

/// Default built-in carrier helper for tests.
#[must_use]
pub fn default_carrier() -> CarrierId {
    CarrierId("1".to_string())
}

fn encode_base64(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    let mut chunks = bytes.chunks_exact(3);
    for chunk in &mut chunks {
        let n = (u32::from(chunk[0]) << 16) | (u32::from(chunk[1]) << 8) | u32::from(chunk[2]);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push(TABLE[(n & 0x3f) as usize] as char);
    }
    let rem = chunks.remainder();
    if rem.len() == 1 {
        let n = u32::from(rem[0]) << 16;
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push('=');
        out.push('=');
    } else if rem.len() == 2 {
        let n = (u32::from(rem[0]) << 16) | (u32::from(rem[1]) << 8);
        out.push(TABLE[((n >> 18) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 12) & 0x3f) as usize] as char);
        out.push(TABLE[((n >> 6) & 0x3f) as usize] as char);
        out.push('=');
    }
    out
}

fn encode_bounded_json(payload: &Value) -> Result<String, Refusal> {
    let text = serde_json::to_string(payload)
        .map_err(|e| Refusal::new(RefusalCode::InvalidDocument, e.to_string()))?;
    if text.len() > MAX_OUTPUT_BYTES {
        return Err(Refusal::new(
            RefusalCode::BudgetExceeded,
            "knitting/dyeing WASM response exceeds 16 MiB budget",
        ));
    }
    Ok(text)
}

/// Browser export wrapping [`knit_swatch_json`] with structured refusal JSON errors.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = knit_swatch_json)]
pub fn web_knit_swatch_json(request_json: &str) -> String {
    match knit_swatch_json(request_json) {
        Ok(json_str) => format!(r#"{{"ok":true,"result":{json_str}}}"#),
        Err(refusal) => serde_json::to_string(&KnitWasmEnvelope {
            ok: false,
            result: None,
            refusal: Some(refusal),
        })
        .unwrap_or_else(|_| {
            r#"{"ok":false,"refusal":{"code":"invalid_document","message":"serialization error"}}"#
                .to_string()
        }),
    }
}

/// Browser export wrapping [`knit_panel_preview_json`] with structured refusal JSON errors.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = knit_panel_preview_json)]
pub fn web_knit_panel_preview_json(request_json: &str) -> String {
    match knit_panel_preview_json(request_json) {
        Ok(json_str) => format!(r#"{{"ok":true,"result":{json_str}}}"#),
        Err(refusal) => serde_json::to_string(&KnitWasmEnvelope {
            ok: false,
            result: None,
            refusal: Some(refusal),
        })
        .unwrap_or_else(|_| {
            r#"{"ok":false,"refusal":{"code":"invalid_document","message":"serialization error"}}"#
                .to_string()
        }),
    }
}

/// Browser export wrapping [`dye_optimize_json`] with structured refusal JSON errors.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(js_name = dye_optimize_json)]
pub fn web_dye_optimize_json(request_json: &str) -> String {
    match dye_optimize_json(request_json) {
        Ok(json_str) => format!(r#"{{"ok":true,"result":{json_str}}}"#),
        Err(refusal) => serde_json::to_string(&KnitWasmEnvelope {
            ok: false,
            result: None,
            refusal: Some(refusal),
        })
        .unwrap_or_else(|_| {
            r#"{"ok":false,"refusal":{"code":"invalid_document","message":"serialization error"}}"#
                .to_string()
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wasm_knit_swatch_and_dye_optimize_round_trip_cleanly() {
        let swatch_req = json!({
            "machine": "kniterate-7e",
            "yarn_id": "merino-200tex",
            "needles": 24,
            "courses_per_zone": 10,
            "cam_settings": [6.0, 8.0]
        })
        .to_string();
        let sw_out: Value = serde_json::from_str(&knit_swatch_json(&swatch_req).unwrap()).unwrap();
        assert_eq!(sw_out["svg_checks_passed"], true);
        assert_eq!(sw_out["cert"]["hardware_execution_authorized"], false);
        assert!(sw_out["kc_text"].as_str().unwrap().contains("GRIPPER_IN 1"));

        let mut pixels = Vec::with_capacity(16);
        for y in 0..4 {
            for _x in 0..4 {
                pixels.push(if y < 2 {
                    [30, 120, 190]
                } else {
                    [220, 160, 40]
                });
            }
        }
        let dye_req = json!({
            "machine": "piezo-cmyk",
            "wearer_age_class": "calibration_swatch_only",
            "regime": "regime1_a_stripes",
            "width": 4,
            "height": 4,
            "srgb_u8": pixels,
            "grid": [6, 6],
            "is_circular": true,
            "drift_eps": 0.002,
            "drift_sigma_eta": 0.015,
            "wick_mm": 1.5,
            "max_iters": 12
        })
        .to_string();
        let dye_out: Value = serde_json::from_str(&dye_optimize_json(&dye_req).unwrap()).unwrap();
        assert_eq!(dye_out["hardware_execution_authorized"], false);
        assert_eq!(dye_out["pareto_regimes"].as_array().unwrap().len(), 5);
        assert!(
            dye_out["controller_gcode"]
                .as_str()
                .unwrap()
                .contains("G1 E")
        );
        assert!(dye_out["controller_export_refusal"].is_null());

        // Verify exact browser knit-page.js panel preview & Pareto fields round-trip cleanly.
        let panel_req = json!({
            "machine": "brother-kh930-ayab",
            "panel": {
                "name": "workbench_panel",
                "wearer_age_class": "infant_under3",
                "hem_width_mm": 250.0,
                "chest_width_mm": 280.0,
                "length_mm": 310.0,
                "armhole_depth_mm": 95.0,
                "armhole_inset_total_mm": 36.0,
                "neck_width_mm": 105.0,
                "neck_depth_mm": 24.0,
                "rib_hem_courses": 6,
                "cam_setting": 7.0,
                "yarn_id": "merino-200tex",
                "carrier_id": "A",
                "has_hood_or_neck_drawstring": false
            },
            "gauge": {
                "wales_per_100mm": 22.0,
                "courses_per_100mm": 30.0,
                "needle_pitch_mm": 4.5,
                "yarn_tex": 200.0,
                "relaxed_after_wash": false
            }
        })
        .to_string();
        let panel_out: Value =
            serde_json::from_str(&knit_panel_preview_json(&panel_req).unwrap()).unwrap();
        assert_eq!(panel_out["svg_checks_passed"], true);
        assert_eq!(panel_out["fit_claimed"], false);
        assert_eq!(panel_out["hardware_execution_authorized"], false);

        let first_pareto = &dye_out["pareto_regimes"][0];
        assert!(first_pareto["expected_delta_e00_p95"].as_f64().is_some());
        assert!(first_pareto["complexity_rank"].as_u64().is_some());
        assert!(first_pareto["feasible"].as_bool().is_some());

        // Refuse out-of-range grid / negative wick / zero max_iters instead of silently clamping.
        let bad_grid = json!({
            "machine": "piezo-cmyk",
            "wearer_age_class": "calibration_swatch_only",
            "regime": "regime1_b_circular_pooling",
            "width": 4,
            "height": 4,
            "srgb_u8": pixels,
            "grid": [0, 6],
            "is_circular": true,
            "drift_eps": 0.002,
            "drift_sigma_eta": 0.015,
            "wick_mm": 1.5,
            "max_iters": 12
        })
        .to_string();
        assert_eq!(
            dye_optimize_json(&bad_grid).unwrap_err().code,
            RefusalCode::BudgetExceeded
        );

        let bad_wick = json!({
            "machine": "piezo-cmyk",
            "wearer_age_class": "calibration_swatch_only",
            "regime": "regime1_b_circular_pooling",
            "width": 4,
            "height": 4,
            "srgb_u8": pixels,
            "grid": [6, 6],
            "is_circular": true,
            "drift_eps": 0.002,
            "drift_sigma_eta": 0.015,
            "wick_mm": -0.5,
            "max_iters": 12
        })
        .to_string();
        assert_eq!(
            dye_optimize_json(&bad_wick).unwrap_err().code,
            RefusalCode::OutOfRange
        );
    }

    #[test]
    fn brother_ayab_swatch_emits_png_base64_and_consistent_cert_material_sha() {
        fn decode_base64(input: &str) -> Vec<u8> {
            let val = |b: u8| -> Option<u8> {
                match b {
                    b'A'..=b'Z' => Some(b - b'A'),
                    b'a'..=b'z' => Some(b - b'a' + 26),
                    b'0'..=b'9' => Some(b - b'0' + 52),
                    b'+' => Some(62),
                    b'/' => Some(63),
                    _ => None,
                }
            };
            let bytes = input.as_bytes();
            assert_eq!(bytes.len() % 4, 0);
            let mut out = Vec::with_capacity(bytes.len() / 4 * 3);
            for chunk in bytes.chunks_exact(4) {
                let a = val(chunk[0]).unwrap();
                let b = val(chunk[1]).unwrap();
                out.push((a << 2) | (b >> 4));
                if chunk[2] != b'=' {
                    let c = val(chunk[2]).unwrap();
                    out.push((b << 4) | (c >> 2));
                    if chunk[3] != b'=' {
                        let d = val(chunk[3]).unwrap();
                        out.push((c << 6) | d);
                    }
                }
            }
            out
        }

        let req = json!({
            "machine": "brother-kh930-ayab",
            "yarn_id": "merino-200tex",
            "needles": 20,
            "courses_per_zone": 8,
            "cam_settings": [4.0, 6.0]
        })
        .to_string();
        let out: Value = serde_json::from_str(&knit_swatch_json(&req).unwrap()).unwrap();
        assert_eq!(
            out["cert"]["material_sha256"].as_str(),
            Some(SWATCH_MATERIAL_PLACEHOLDER)
        );
        let b64 = out["ayab_png_base64"].as_str().unwrap();
        let png = decode_base64(b64);
        assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"));
        let sha = ftx_core::receipt::sha256_hex(&png);
        assert_eq!(out["ayab_png_sha256"].as_str(), Some(sha.as_str()));
        assert_eq!(
            out["ayab_checklist"]["png_sha256"].as_str(),
            Some(sha.as_str())
        );
    }

    #[test]
    fn flatbed_dye_optimize_orders_predicted_srgb_in_canonical_course_wale_grid() {
        // Left half crisp blue, right half crisp gold across all rows.
        // In flatbed boustrophedon knitting, odd courses traverse right-to-left along the yarn,
        // so returning raw yarn-path order would mirror odd rows horizontally!
        let mut pixels = Vec::with_capacity(36);
        for _y in 0..6 {
            for x in 0..6 {
                pixels.push(if x < 3 {
                    [25, 95, 195]
                } else {
                    [225, 175, 40]
                });
            }
        }
        let req = json!({
            "machine": "piezo-cmyk",
            "wearer_age_class": "calibration_swatch_only",
            "regime": "regime2_fiducial_synced",
            "width": 6,
            "height": 6,
            "srgb_u8": pixels,
            "grid": [4, 6],
            "is_circular": false,
            "drift_eps": 0.0,
            "drift_sigma_eta": 0.0,
            "wick_mm": 0.2,
            "max_iters": 16
        })
        .to_string();
        let out: Value = serde_json::from_str(&dye_optimize_json(&req).unwrap()).unwrap();
        let grid = out["predicted_stitch_srgb"].as_array().unwrap();
        assert_eq!(grid.len(), 24);
        for c in 0..4 {
            let left = grid[c * 6].as_array().unwrap();
            let right = grid[c * 6 + 5].as_array().unwrap();
            let left_b_minus_r = left[2].as_i64().unwrap() - left[0].as_i64().unwrap();
            let right_b_minus_r = right[2].as_i64().unwrap() - right[0].as_i64().unwrap();
            assert!(
                left_b_minus_r > right_b_minus_r,
                "course {c} should be bluer on the left than the right (left={left:?}, right={right:?})"
            );
        }
    }
}
