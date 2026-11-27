//! Cross-crate conformance, safety, physics KAT, and determinism tests for computational knitting
//! and programmed continuous length-wise yarn dyeing (`FTX-K01`..`FTX-K19`).
//!
//! Explicitly separates the Four Claims:
//! - Format contracts (Knitout v2, Kniterate pass-oriented KCode, Brother AYAB PNG, CYC SVG/PDF),
//! - Physics KATs (Poincloux et al. 2018 Rung-3 elastica, exact Kubelka-Munk / Sharma 2005 CIEDE2000,
//!   conserved-mass capillary transport + VJP, Sobol-CVaR Bertsekas L-BFGS),
//! - Honesty & safety invariants (child/infant EN 14682 / float / eyelet / tail / fastness gates,
//!   Tier-B proprietary target refusal, `hardware_execution_authorized: false`, `fit_claimed: false`),
//! - Bit-exact determinism across repeated executions.

use std::collections::BTreeMap;

use ftx_core::det;
use ftx_core::receipt::canonical_json_sha256;
use ftx_core::shaping::{BoundaryRing, EaseShellMesh};
use ftx_core::{RefusalCode, TargetRaster, Vec3};
use ftx_drape::dye::{
    CapillaryTransportConfig, CieLab, CieXyz, DriftParameters, DyeInverseConfig, DyeOpticsModel,
    DyeUncertaintyContext, KnitStitchMap, ciede2000_exact, ciede2000_sq_smooth_grad,
    coherent_open_loop_horizons, forward_capillary_transport, linear_xyz_to_lab,
    optimize_yarn_dye_program, optimize_yarn_dye_program_with_context, rank_five_regime_pareto,
    srgb_u8_to_xyz_d65, vjp_capillary_transport,
};
use ftx_drape::knit::{
    AsymmetricKnitBending, CoupledKnitMembrane, KnitHysteresisBank, LocalAuditPatchInput,
    PoinclouxParams, RveHomogenizedKnitCard, assess_knit_shell_state, audit_local_knit_patch,
    calibrate_knit_prior_to_uniaxial_strips, cayley_hamilton_inv_sqrt_2x2,
    poincloux_uniaxial_curve,
};
use ftx_export::dye::{verify_and_export_dye_program, verify_and_export_dye_program_with_receipt};
use ftx_export::knit::{
    TierBProprietaryTarget, VendorHandoffManifest, check_hand_knit_pdf, check_hand_knit_svg,
    export_brother_ayab, export_calibration_swatch_kit, export_hand_knit_document,
    export_kniterate_kc, export_knitout_v2, export_tier_b_vendor_handoff,
    refuse_proprietary_binary_target, verify_tier_b_handoff_bundle,
};
use ftx_pattern::dye::{
    DYE_PROGRAM_SCHEMA, DYE_UNCERTAINTY_SCHEMA, DancerAccumulatorSpec, DyeMachineProfile,
    DyePlanUncertaintyReceipt, DyeSegment, DyeingRegime, IntakeEvent, IntakeEventKind,
    OpticalMeasurementBasis, Regime3IntakeTimeline, YarnDyeProgram, YarnDyeingQualification,
};
use ftx_pattern::knit::{
    AutoKnitConfig, Bed, CarrierId, GaugeSpec, KNITOUT_SCHEMA, KnitSafetyContext,
    KnitSwatchQualification, KnitoutHeader, KnitoutOp, KnitoutProgram, MachineProfile, Needle,
    ParametricPanelSpec, RaglanPulloverSpec, WearerAgeClass, compile_knit_doc,
    draft_autoknit_from_shell, draft_calibration_swatch, draft_seamless_raglan_tube,
    draft_shaped_panel, parse_knitout, verify_knitout_program,
};
use ftx_pattern::materials::{Evidence, Source};
use ftx_trace::raster::load_target_raster_with_limit;

fn default_gauge() -> GaugeSpec {
    GaugeSpec {
        wales_per_100mm: 22.0,
        courses_per_100mm: 30.0,
        needle_pitch_mm: 25.4 / 7.0,
        yarn_tex: Some(200.0),
        thickness_mm: Some(2.0),
        relaxed_after_wash: true,
    }
}

fn synthetic_swatch_qual(profile: &MachineProfile, yarn_id: &str) -> KnitSwatchQualification {
    KnitSwatchQualification {
        id: format!("swatch-{}-{yarn_id}", profile.id),
        material_sha256: "test-yarn-lot-sha256".to_string(),
        machine_sha256: profile.canonical_sha256().unwrap(),
        yarn_lot_id: "lot-test-001".to_string(),
        was_washed_and_blocked: true,
        wales_per_100mm: 22.0,
        courses_per_100mm: 30.0,
        thickness_mm: 2.0,
        cam_stitch_setting: 7.0,
        wick_dispersion_mm: None,
        shrinkage_axial_pct: None,
        yarn_intake_drift_eps: Some(0.002),
        yarn_intake_drift_sigma_eta: Some(0.015),
        evidence: Evidence::Measured,
        source: Source {
            reference: "local:synthetic_conformance_swatch".to_string(),
            accessed_on: "2026-11-27".to_string(),
            note: "Synthetic washed swatch qualification for conformance testing".to_string(),
        },
        method: "Washed, blocked flat 24h, measured over 100mm x 100mm".to_string(),
        conditioning: BTreeMap::new(),
        allow_estimates: false,
    }
}

#[test]
fn det_cbrt_and_canonical_json_sha256_invariants() {
    for &x in &[-1000.0, -1.0, -1e-9, -0.0, 0.0, 1e-12, 0.008856, 1.0, 27.0] {
        let c = det::cbrt(x);
        if x == 0.0 {
            assert_eq!(c, 0.0);
        } else {
            let rel = ((c * c * c) - x).abs() / x.abs();
            assert!(rel < 1e-14, "cbrt({x}) = {c}, rel={rel}");
        }
    }
    let a = serde_json::json!({"b": -0.0, "a": [1, {"z": 2, "y": 3}]});
    let b = serde_json::json!({"a": [1, {"y": 3, "z": 2}], "b": 0.0});
    assert_eq!(
        canonical_json_sha256(&a).unwrap(),
        canonical_json_sha256(&b).unwrap()
    );
}

#[test]
fn target_raster_preserves_straight_alpha_png() {
    // Minimal 2x1 RGBA PNG with one transparent black pixel (alpha=0) and one half-alpha red pixel
    let png = build_test_rgba_png(2, 1, &[0, 0, 0, 0, 200, 40, 20, 128]);
    let raster = load_target_raster_with_limit(&png, 1024).unwrap();
    assert_eq!(raster.width, 2);
    assert_eq!(raster.height, 1);
    assert_eq!(raster.srgb_u8[0], [0, 0, 0]);
    assert!(raster.alpha_weight[0] < 1e-6);
    assert_eq!(raster.srgb_u8[1], [200, 40, 20]);
    assert!((raster.alpha_weight[1] - (128.0 / 255.0)).abs() < 1e-4);
}

#[test]
fn knitting_end_to_end_swatch_panel_tube_and_format_boundaries() {
    let kniterate = MachineProfile::kniterate_7e();
    let ayab = MachineProfile::brother_kh930_ayab();
    let gauge = default_gauge();
    let qual = synthetic_swatch_qual(&kniterate, "merino-200tex");

    // 1. Calibration swatch kit (Exp-3) passes without prior qualification + lossless read-back
    let kit = export_calibration_swatch_kit(
        &kniterate,
        "merino-200tex",
        "lot-01",
        28,
        10,
        &[5.0, 7.0, 9.0],
    )
    .unwrap();
    assert!(!kit.knitout.cert.hardware_execution_authorized);
    assert!(!kit.knitout.cert.physical_knitting_validated);
    assert!(!kit.knitout.cert.four_claims.fit_claimed);
    let reparsed = parse_knitout(&kit.knitout.knitout_text).unwrap();
    assert_eq!(
        reparsed.to_knitout_text().unwrap(),
        kit.knitout.knitout_text
    );

    // 2. Measured shaped panel for toddler refuses without Exp-3 swatch qualification on garment export
    let panel_spec = ParametricPanelSpec {
        name: "toddler_sweater_front".to_string(),
        wearer_age_class: WearerAgeClass::InfantUnder3,
        hem_width_mm: 240.0,
        chest_width_mm: 270.0,
        length_mm: 290.0,
        armhole_depth_mm: 90.0,
        armhole_inset_total_mm: 36.0,
        neck_width_mm: 0.0,
        neck_depth_mm: 0.0,
        rib_hem_courses: 6,
        cam_setting: 7.0,
        yarn_id: "merino-200tex".to_string(),
        carrier_id: CarrierId("1".to_string()),
        has_hood_or_neck_drawstring: false,
    };
    let panel_doc = draft_shaped_panel(&panel_spec, &gauge, &kniterate).unwrap();
    let err_unqual = compile_knit_doc(&panel_doc, &kniterate, None, None, None).unwrap_err();
    assert_eq!(err_unqual.code, RefusalCode::UnqualifiedProcess);

    // A matching synthetic swatch qualifies the supported panel and verified Knitout.
    // The native pass backend retains source identity and independent KCode read-back.
    let compiled = compile_knit_doc(
        &panel_doc,
        &kniterate,
        Some(&qual),
        Some("test-yarn-lot-sha256"),
        None,
    )
    .unwrap();
    let ko = export_knitout_v2(&compiled).unwrap();
    let kc = export_kniterate_kc(&compiled, &kniterate).unwrap();
    assert_eq!(kc.verification.program_sha256, compiled.cert.program_sha256);
    assert!(kc.kc_text.starts_with("HOME\nRACK:0\n"));
    assert!(!kc.verification.hardware_execution_authorized);
    assert!(!ko.cert.hardware_execution_authorized);

    let hand = export_hand_knit_document(&panel_doc, "2026-11-27T08:00").unwrap();
    assert!(
        check_hand_knit_svg(&hand.svg_text, "panel.svg")
            .iter()
            .all(|c| c.ok)
    );
    assert!(
        check_hand_knit_pdf(&hand.pdf_bytes, "panel.pdf")
            .iter()
            .all(|c| c.ok)
    );

    // 3. Brother AYAB 1-bit PNG + checklist
    let ayab_doc =
        draft_calibration_swatch(&ayab, "fingering-100tex", 24, 12, &[5.0, 7.0]).unwrap();
    let ayab_compiled = compile_knit_doc(&ayab_doc, &ayab, None, None, None).unwrap();
    let ayab_bundle = export_brother_ayab(&ayab_doc, &ayab, &ayab_compiled.cert).unwrap();
    assert!(ayab_bundle.png_bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(ayab_bundle.checklist.width_needles, 24);
    assert!(!ayab_bundle.checklist.automated_carriage_control);
    assert!(!ayab_bundle.cert.hardware_execution_authorized);

    // 4. EN 14682 child drawstring ban refused on both machine compilation and hand-knit export
    let mut bad_panel = panel_spec.clone();
    bad_panel.has_hood_or_neck_drawstring = true;
    let bad_doc = draft_shaped_panel(&bad_panel, &gauge, &kniterate).unwrap();
    let err_drawstring = compile_knit_doc(
        &bad_doc,
        &kniterate,
        Some(&qual),
        Some("test-yarn-lot-sha256"),
        None,
    )
    .expect_err("EN 14682 must refuse child drawstring");
    assert_eq!(err_drawstring.code, RefusalCode::MaterialProcessHazard);
    let err_hand_drawstring = export_hand_knit_document(&bad_doc, "2026-11-27T08:00")
        .expect_err("EN 14682 must refuse child drawstring in hand-knit chart export");
    assert_eq!(err_hand_drawstring.code, RefusalCode::MaterialProcessHazard);

    // 5. Tier-B proprietary targets (Shima .dat & Stoll SINTRAL) refuse direct binary emission
    //    with exact ranked repair text and emit sealed, VM-verifiable Knitout v2 vendor handoffs.
    let expected_repair = "Export verified Knitout v2 + vendor handoff manifest for licensed Shima SDS-ONE APEX / Stoll M1plus compilation.";
    for (profile, target, fmt) in [
        (
            MachineProfile::shima_seiki_12e_handoff(),
            TierBProprietaryTarget::ShimaSeikiDat,
            ".dat",
        ),
        (
            MachineProfile::stoll_12e_handoff(),
            TierBProprietaryTarget::StollSintral,
            "SINTRAL",
        ),
    ] {
        assert_eq!(target.withheld_format(), fmt);
        let err_tier_b = refuse_proprietary_binary_target(&profile, fmt).unwrap_err();
        assert_eq!(err_tier_b.code, RefusalCode::OpaqueProprietaryTarget);
        assert_eq!(
            err_tier_b.repairs.first().map(String::as_str),
            Some(expected_repair)
        );

        let tb_doc =
            draft_calibration_swatch(&profile, "merino-120tex", 18, 6, &[5.0, 6.0]).unwrap();
        let tb_compiled =
            compile_knit_doc(&tb_doc, &profile, None, Some("mat-tierb-sha"), None).unwrap();
        let handoff = export_tier_b_vendor_handoff(&tb_compiled, &profile, fmt).unwrap();
        assert!(!handoff.manifest.machine_executable);
        assert!(!handoff.manifest.hardware_execution_authorized);
        assert!(!handoff.manifest.physical_knitting_validated);
        assert!(!handoff.manifest.fit_claimed);
        assert_eq!(handoff.manifest_sha256, handoff.manifest.manifest_sha256);

        let tb_ctx = KnitSafetyContext {
            wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
            blocked_wale_pitch_mm: 100.0 / tb_doc.gauge.wales_per_100mm,
            source_doc: Some(&tb_doc),
            speed_fraction: Some(0.50),
            takedown_fraction: Some(0.60),
        };
        let vm_report = verify_tier_b_handoff_bundle(&handoff, &profile, &tb_ctx).unwrap();
        assert_eq!(vm_report.program_sha256, handoff.knitout.knitout_sha256);

        let mut tampered_manifest = handoff.manifest.clone();
        tampered_manifest.physical_knitting_validated = true;
        assert_eq!(
            tampered_manifest.verify_integrity().unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
        let mut tampered_bundle = handoff;
        tampered_bundle.manifest.fit_claimed = true;
        assert_eq!(
            verify_tier_b_handoff_bundle(&tampered_bundle, &profile, &tb_ctx)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
        // JSON round-trip check
        let rt: VendorHandoffManifest =
            serde_json::from_str(&tampered_bundle.manifest_json).unwrap();
        rt.verify_integrity().unwrap();
    }

    // 6. The supported shaped torso tube compiles deterministically, with both beds retained.
    let raglan = RaglanPulloverSpec {
        name: "toddler_raglan".to_string(),
        wearer_age_class: WearerAgeClass::InfantUnder3,
        neck_girth_mm: 260.0,
        chest_girth_mm: 520.0,
        yoke_depth_mm: 120.0,
        body_length_below_arm_mm: 180.0,
        collar_rib_courses: 0,
        hem_rib_courses: 0,
        cam_setting: 7.0,
        yarn_id: "merino-200tex".to_string(),
        carrier_id: CarrierId("1".to_string()),
        has_hood_or_neck_drawstring: false,
    };
    let rag_doc = draft_seamless_raglan_tube(&raglan, &gauge, &kniterate).unwrap();
    let rag1 = compile_knit_doc(
        &rag_doc,
        &kniterate,
        Some(&qual),
        Some("test-yarn-lot-sha256"),
        None,
    )
    .unwrap();
    let rag2 = compile_knit_doc(
        &rag_doc,
        &kniterate,
        Some(&qual),
        Some("test-yarn-lot-sha256"),
        None,
    )
    .unwrap();
    assert_eq!(rag1.cert.program_sha256, rag2.cert.program_sha256);
    assert_eq!(ko.knitout_sha256, compiled.cert.program_sha256);

    // 7. VM refuses slider ops on Kniterate 7E (no sliders)
    let slider_prog = KnitoutProgram {
        schema: KNITOUT_SCHEMA.to_string(),
        header: KnitoutHeader {
            machine: kniterate.name.clone(),
            gauge: format!("{}", kniterate.gauge_e),
            width: kniterate.needle_count,
            position: "Center".to_string(),
            carriers: vec![CarrierId("1".to_string())],
            extra: BTreeMap::new(),
        },
        ops: vec![
            KnitoutOp::In {
                carriers: vec![CarrierId("1".to_string())],
            },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 1),
                to: Needle::new(Bed::FrontSlider, 1),
            },
        ],
    };
    let vm_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::Adult,
        blocked_wale_pitch_mm: gauge.blocked_wale_pitch_mm(),
        source_doc: None,
        speed_fraction: Some(0.5),
        takedown_fraction: Some(0.5),
    };
    let err_slider = verify_knitout_program(&slider_prog, &kniterate, &vm_ctx).unwrap_err();
    assert_eq!(err_slider.code, RefusalCode::MachineStateInfeasible);
}

#[test]
fn autoknit_pathway2_drafts_harmonic_frustum_shell() {
    let rings = 6usize;
    let segs = 16usize;
    let mut vertices_m = Vec::new();
    let mut triangles = Vec::new();
    for r in 0..=rings {
        let z = 0.05 + 0.25 * (r as f64 / rings as f64);
        let rad = 0.085 - 0.015 * (r as f64 / rings as f64);
        for s in 0..segs {
            let theta = 2.0 * std::f64::consts::PI * (s as f64 / segs as f64);
            let p = Vec3::new(rad * det::cos(theta), rad * det::sin(theta), z);
            vertices_m.push([p.x, p.y, p.z]);
        }
    }
    for r in 0..rings {
        for s in 0..segs {
            let sn = (s + 1) % segs;
            let a = (r * segs + s) as u32;
            let b = (r * segs + sn) as u32;
            let c = ((r + 1) * segs + sn) as u32;
            let d = ((r + 1) * segs + s) as u32;
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    let start_ring: Vec<u32> = (0..segs as u32).collect();
    let end_ring: Vec<u32> = ((rings * segs) as u32..((rings + 1) * segs) as u32).collect();
    let shell = EaseShellMesh {
        vertices_m,
        triangles,
        boundary_rings: vec![
            BoundaryRing {
                name: "hem".to_string(),
                is_start: true,
                vertices: start_ring,
                perimeter_m: 2.0 * std::f64::consts::PI * 0.085,
            },
            BoundaryRing {
                name: "neck".to_string(),
                is_start: false,
                vertices: end_ring,
                perimeter_m: 2.0 * std::f64::consts::PI * 0.070,
            },
        ],
        source_body_sha256: "synthetic-frustum-sha256".to_string(),
    };
    let profile = MachineProfile::kniterate_7e();
    let cfg = AutoKnitConfig {
        name: "frustum_tube".to_string(),
        wearer_age_class: WearerAgeClass::InfantUnder3,
        yarn_id: "merino-200tex".to_string(),
        carrier_id: CarrierId("1".to_string()),
        cam_setting: 7.0,
        max_idt_flips: 128,
        has_hood_or_neck_drawstring: false,
    };
    let res = draft_autoknit_from_shell(&shell, &default_gauge(), &profile, &cfg).unwrap();
    assert!(res.min_cotangent_weight > -1e-6);
    assert!(res.doc.stitches.len() > 200);
    assert!(res.max_abs_girth_residual_mm < 15.0);
}

#[test]
fn knit_constitutive_poincloux_kat_prior_calibration_and_honesty_flags() {
    let mem = CoupledKnitMembrane::reference_stockinette_estimate();
    let eval = mem.evaluate([0.12, 0.08, 0.04]).unwrap();
    assert!(eval.energy_density > 0.0);
    assert!(eval.stress[0] > 0.0 && eval.tangent[0][0] > 0.0);
    // Exceeding jamming limit refuses cleanly with OutOfRange
    assert_eq!(
        mem.evaluate([mem.e1_jam + 0.05, 0.0, 0.0])
            .unwrap_err()
            .code,
        RefusalCode::OutOfRange
    );

    // Cayley-Hamilton C^{-1/2} identity check: M * C * M = I
    let (c00, c01, c11) = (1.44, 0.12, 1.21);
    let m = cayley_hamilton_inv_sqrt_2x2(c00, c01, c11).unwrap();
    let mc00 = m[0] * c00 + m[1] * c01;
    let mc01 = m[0] * c01 + m[1] * c11;
    let mc10 = m[1] * c00 + m[2] * c01;
    let mc11 = m[1] * c01 + m[2] * c11;
    let id00 = mc00 * m[0] + mc01 * m[1];
    let id01 = mc00 * m[1] + mc01 * m[2];
    let id11 = mc10 * m[1] + mc11 * m[2];
    assert!((id00 - 1.0).abs() < 1e-12);
    assert!(id01.abs() < 1e-12);
    assert!((id11 - 1.0).abs() < 1e-12);

    // Asymmetric bending strictly positive curvature energy + Prandtl-Ishlinskii trial immutability
    let bend = AsymmetricKnitBending::reference_stockinette_estimate();
    let b_eval = bend.evaluate_from_kappa([4.0, -3.0, 1.5]);
    assert!(b_eval.energy_density > 0.0);
    let hyst = KnitHysteresisBank {
        thresholds: vec![0.02, 0.06],
        weights: vec![25.0, 15.0],
        huber_eps: 1e-4,
    };
    let s0 = hyst.zero_state();
    let (sig1, _d1, next1) = hyst.evaluate_trial([0.15, 0.05, 0.0], &s0);
    let (sig2, _d2, _) = hyst.evaluate_trial([0.15, 0.05, 0.0], &s0);
    assert_eq!(sig1, sig2);
    assert_ne!(next1.play_backstrain, s0.play_backstrain);

    // Rung-3 Poincloux et al. (2018) planar elastica KAT
    let p_res = poincloux_uniaxial_curve(
        &PoinclouxParams::reference_monofilament(),
        &[0.0, 0.05, 0.15, 0.25, 0.32],
    )
    .unwrap();
    assert_eq!(p_res.len(), 5);
    assert!(p_res[4].tension_loading_n_m > p_res[4].tension_frictionless_n_m);
    assert!(p_res[4].tension_unloading_n_m < p_res[4].tension_frictionless_n_m);
    assert!(p_res[3].strain_course_free < 0.0);

    // RVE card + honest uniaxial strip calibration (FTX-K11) + local audit patch
    let card = RveHomogenizedKnitCard {
        schema: "ftx-knit-rve-card/1".to_string(),
        id: "jersey-rve-est".to_string(),
        frankensim_rve_sha256: "abc123".to_string(),
        evidence: Evidence::Estimate,
        hill_mandel_residual: 4.2e-7,
        membrane: mem,
        bending: bend,
        rest_pore_fraction: 0.08,
        pore_strain_slope: 0.35,
    };
    let wale_obs: Vec<(f64, f64)> = [0.10, 0.20, 0.30]
        .iter()
        .map(|&e| {
            (
                e,
                1.4 * card.membrane.evaluate([e, 0.0, 0.0]).unwrap().stress[0],
            )
        })
        .collect();
    let course_obs: Vec<(f64, f64)> = [0.10, 0.20, 0.30]
        .iter()
        .map(|&e| {
            (
                e,
                0.8 * card.membrane.evaluate([0.0, e, 0.0]).unwrap().stress[1],
            )
        })
        .collect();
    let cal = calibrate_knit_prior_to_uniaxial_strips(&card, &wale_obs, &course_obs).unwrap();
    assert_eq!(cal.card.evidence, Evidence::Estimate);
    assert!((cal.wale_scale - 1.4).abs() < 0.05);
    assert!((cal.course_scale - 0.8).abs() < 0.05);

    let audit = audit_local_knit_patch(
        &cal.card,
        &LocalAuditPatchInput {
            macro_strain: [0.15, 0.10, 0.02],
            macro_curvature: [14.0, -15.0, 1.0],
            patch_span_m: 0.04,
        },
        0.0045,
    )
    .unwrap();
    assert!(audit.energy_density_j_m2 > 0.0);
    assert!(audit.pore_open_fraction > 0.08);

    // Honesty flags + Seam-4 DrapeUncertaintyReceipt
    let assess = assess_knit_shell_state(
        "pat-sha".to_string(),
        "mat-sha".to_string(),
        0.55,
        0.02, // < 0.05 -> KnitJamming
        0.45, // > 0.35 -> KnitEdgeCurl
        0.30, // > 0.25 -> KnitRibSnap
        0.25, // > 0.20 -> KnitPoreGrinThrough
        0.04, // > 0.02 -> KnitHysteresisDrift
        0.10,
        Evidence::Estimate,
    )
    .unwrap();
    assert_eq!(assess.flags.len(), 5);
    assert_eq!(assess.receipt.gating_flags.len(), 5);
}

#[test]
fn dye_optics_transport_vjp_inverse_cvar_and_verifier_end_to_end() {
    // 1. Sharma (2005) CIEDE2000 reference pair #1 (2.0425) + smooth white-yarn finite grad
    let de1 = ciede2000_exact(
        CieLab {
            l: 50.0,
            a: 2.6772,
            b: -79.7751,
        },
        CieLab {
            l: 50.0,
            a: 0.0,
            b: -82.7485,
        },
    );
    assert!((de1 - 2.0425).abs() < 5e-4, "de1={de1}");
    let white = CieLab {
        l: 95.0,
        a: 0.0,
        b: 0.0,
    };
    let (sq0, grad0) = ciede2000_sq_smooth_grad(white, white, 1e-4);
    assert!(sq0.is_finite() && grad0.iter().all(|g| g.is_finite()));
    let (white_lab, _) = linear_xyz_to_lab(CieXyz {
        x: 0.95047,
        y: 1.0,
        z: 1.08883,
    });
    assert!((white_lab.l - 100.0).abs() < 1e-4);

    // 2. Conserved-mass capillary transport + exact adjoint VJP dot-product check
    let tr_cfg = CapillaryTransportConfig {
        ds_m: 0.01,
        sigma_wick_m: 0.004,
        rf_retardations: vec![1.0, 0.85, 0.70, 0.60],
        fixation_efficiency: vec![0.95; 4],
        shrinkage_axial_frac: 0.03,
    };
    let n_cells = 16;
    let n_p = 4;
    let mut u = vec![0.0; n_cells * n_p];
    for c in 4..10 {
        for p in 0..n_p {
            u[c * n_p + p] = 0.25 * (p as f64 + 1.0);
        }
    }
    let c_out = forward_capillary_transport(&u, n_cells, &tr_cfg).unwrap();
    let w: Vec<f64> = (0..c_out.fixed_conc.len())
        .map(|i| ((i % 7) as f64 - 3.0) * 0.1)
        .collect();
    let vjp = vjp_capillary_transport(&w, n_cells, &tr_cfg).unwrap();
    let lhs: f64 = c_out.fixed_conc.iter().zip(&w).map(|(a, b)| a * b).sum();
    let rhs: f64 = u.iter().zip(&vjp).map(|(a, b)| a * b).sum();
    assert!(
        (lhs - rhs).abs() < 1e-11,
        "VJP dot-product mismatch: {lhs} vs {rhs}"
    );

    // 3. Coherent horizons obey 2x..4x flatbed vs circular penalty (2x systematic, 4x Brownian)
    let drift = DriftParameters {
        systematic_eps: 0.002,
        stochastic_sigma_eta: 0.015,
    };
    let (h_circ, h_flat) = coherent_open_loop_horizons(drift);
    assert!(
        h_circ > h_flat * 2,
        "mixed drift: circ={h_circ}, flat={h_flat}"
    );
    let (h_circ_b, h_flat_b) = coherent_open_loop_horizons(DriftParameters {
        systematic_eps: 0.0,
        stochastic_sigma_eta: 0.015,
    });
    assert!(
        h_circ_b >= h_flat_b * 4,
        "pure Brownian drift must pay 4x flatbed penalty: circ={h_circ_b}, flat={h_flat_b}"
    );

    // 4. Full 4-stage Sobol-CVaR Bertsekas L-BFGS optimization + convergence + bit-exact repeatability
    let mut pixels = Vec::with_capacity(16);
    for y in 0..4 {
        for _x in 0..4 {
            pixels.push(if y < 2 {
                [25, 110, 185]
            } else {
                [220, 155, 45]
            });
        }
    }
    let target = TargetRaster::from_opaque_srgb(4, 4, pixels).unwrap();
    let stitch_map = KnitStitchMap::regular_grid(0, 6, 6, 0.010, true, 0.05);
    let machine = DyeMachineProfile::benchtop_piezo_cmyk();
    let optics = DyeOpticsModel::synthetic_cmyk_cotton();
    let opt_tr_cfg = CapillaryTransportConfig {
        ds_m: stitch_map.total_yarn_length_m / 24.0,
        sigma_wick_m: 0.002,
        rf_retardations: vec![0.85, 0.80, 0.90, 0.72],
        fixation_efficiency: vec![0.95; 4],
        shrinkage_axial_frac: 0.02,
    };
    let inv_cfg = DyeInverseConfig {
        name: "conformance_dye_test".to_string(),
        yarn_id: "cotton-120tex".to_string(),
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        regime: DyeingRegime::Regime1AStripes,
        n_cells: 24,
        n_sobol_scenarios: 8,
        cvar_alpha: 0.90,
        cvar_tau: 0.5,
        lambda_smooth: 0.05,
        lambda_switch: 0.02,
        max_iters: 20,
        skin_underlayer_xyz: srgb_u8_to_xyz_d65([215, 180, 155]),
        ciede_eps_c: 1e-4,
        drift,
    };
    let res1 = optimize_yarn_dye_program(
        &target,
        &stitch_map,
        &machine,
        &optics,
        &opt_tr_cfg,
        &inv_cfg,
    )
    .unwrap();
    let res2 = optimize_yarn_dye_program(
        &target,
        &stitch_map,
        &machine,
        &optics,
        &opt_tr_cfg,
        &inv_cfg,
    )
    .unwrap();
    assert!(
        res1.delta_e00_p50 < 15.0,
        "expected strong perceptual convergence, got p50={}",
        res1.delta_e00_p50
    );
    assert_eq!(res1.predicted_stitch_srgb, res2.predicted_stitch_srgb);
    assert_eq!(res1.delta_e00_p50.to_bits(), res2.delta_e00_p50.to_bits());

    let pareto = rank_five_regime_pareto(&target, &stitch_map, &machine, drift, 4);
    assert_eq!(pareto.len(), 5);

    // 5. Swatch verification & export produces CSV + G-code + sealed cert + linked Seam-4 receipt
    assert_eq!(res1.uncertainty_receipt.schema, DYE_UNCERTAINTY_SCHEMA);
    assert_eq!(
        res1.uncertainty_receipt.payload_sha256,
        res2.uncertainty_receipt.payload_sha256
    );
    assert!(!res1.uncertainty_receipt.four_claims.color_fidelity_claimed);
    assert!(
        !res1
            .uncertainty_receipt
            .four_claims
            .visual_realization_swatch_verified
    );
    assert!(
        !res1
            .uncertainty_receipt
            .four_claims
            .hardware_execution_authorized
    );
    assert!(
        !res1
            .uncertainty_receipt
            .four_claims
            .physical_dyeing_validated
    );
    assert!(!res1.uncertainty_receipt.four_claims.fit_claimed);
    res1.uncertainty_receipt.verify_integrity().unwrap();

    let bundle = verify_and_export_dye_program_with_receipt(
        &res1.program,
        &machine,
        None,
        None,
        Some(&res1.uncertainty_receipt),
    )
    .unwrap();
    assert!(!bundle.cert.hardware_execution_authorized);
    assert!(!bundle.cert.physical_dyeing_validated);
    assert_eq!(
        bundle.cert.uncertainty_receipt_sha256.as_deref(),
        Some(res1.uncertainty_receipt.payload_sha256.as_str())
    );
    assert!(bundle.controller_gcode.contains("G1 E"));
    bundle.cert.verify_integrity().unwrap();

    let mut tampered_receipt = res1.uncertainty_receipt.clone();
    tampered_receipt.four_claims.color_fidelity_claimed = true;
    assert_eq!(
        verify_and_export_dye_program_with_receipt(
            &res1.program,
            &machine,
            None,
            None,
            Some(&tampered_receipt),
        )
        .unwrap_err()
        .code,
        RefusalCode::VerificationMismatch
    );

    // 5b. Calibrated scanner + Measured Exp-4 qualification links Seam-4 receipt end-to-end
    let cal_qual = YarnDyeingQualification {
        id: "dq-conformance-scanner".to_string(),
        material_sha256: "cotton-120tex-sha".to_string(),
        dye_machine_sha256: machine.canonical_sha256().unwrap(),
        yarn_lot_id: "lot-y1".to_string(),
        dye_lot_id: "lot-d1".to_string(),
        wick_dispersion_mm: 2.0,
        shrinkage_axial_pct: 2.0,
        wet_tenacity_retention_ratio: 0.92,
        yarn_breaking_force_n: 8.5,
        yarn_tg_c: None,
        optical_measurement_basis: OpticalMeasurementBasis::CalibratedChartScanner,
        washing_off_validated: true,
        iso_105_c06_grade: Some(4.5),
        iso_105_x12_wet_rub_grade: Some(4.5),
        saliva_fastness_validated: true,
        evidence: Evidence::Measured,
        source: Source {
            reference: "local:conformance_exp4_scanner".to_string(),
            accessed_on: "2026-11-27".to_string(),
            note: "Calibrated chart scanner Exp-4 coupon".to_string(),
        },
        method: "reactive fixation + soap wash + IT8 scanner".to_string(),
        conditioning: BTreeMap::new(),
        allow_estimates: false,
    };
    let mut cal_inv_cfg = inv_cfg.clone();
    cal_inv_cfg.wearer_age_class = WearerAgeClass::Adult;
    let cal_ctx = DyeUncertaintyContext::from_qualification(&cal_qual).unwrap();
    let cal_res = optimize_yarn_dye_program_with_context(
        &target,
        &stitch_map,
        &machine,
        &optics,
        &opt_tr_cfg,
        &cal_inv_cfg,
        &cal_ctx,
    )
    .unwrap();
    assert!(
        cal_res
            .uncertainty_receipt
            .four_claims
            .color_fidelity_claimed
    );
    assert!(
        !cal_res
            .uncertainty_receipt
            .four_claims
            .visual_realization_swatch_verified
    );
    let receipt_json = serde_json::to_string(&cal_res.uncertainty_receipt).unwrap();
    let receipt_rt: DyePlanUncertaintyReceipt = serde_json::from_str(&receipt_json).unwrap();
    receipt_rt.verify_integrity().unwrap();
    let cal_bundle = verify_and_export_dye_program_with_receipt(
        &cal_res.program,
        &machine,
        Some(&cal_qual),
        Some("cotton-120tex-sha"),
        Some(&receipt_rt),
    )
    .unwrap();
    assert_eq!(
        cal_bundle.cert.uncertainty_receipt_sha256.as_deref(),
        Some(receipt_rt.payload_sha256.as_str())
    );

    // 6. Infant wearable refuses without Exp-4 receipt and without saliva fastness, passes when complete
    let mut infant_prog = res1.program.clone();
    infant_prog.wearer_age_class = WearerAgeClass::InfantUnder3;
    infant_prog.not_for_garment_wear = false;
    assert_eq!(
        verify_and_export_dye_program(&infant_prog, &machine, None, None)
            .unwrap_err()
            .code,
        RefusalCode::UnqualifiedProcess
    );
    let mut qual_ok = YarnDyeingQualification {
        id: "dq-conformance-1".to_string(),
        material_sha256: "cotton-120tex-sha".to_string(),
        dye_machine_sha256: machine.canonical_sha256().unwrap(),
        yarn_lot_id: "lot-y1".to_string(),
        dye_lot_id: "lot-d1".to_string(),
        wick_dispersion_mm: 1.4,
        shrinkage_axial_pct: 2.0,
        wet_tenacity_retention_ratio: 0.92,
        yarn_breaking_force_n: 8.5,
        yarn_tg_c: None,
        optical_measurement_basis: OpticalMeasurementBasis::SpectrophotometerD65_10Deg,
        washing_off_validated: true,
        iso_105_c06_grade: Some(4.5),
        iso_105_x12_wet_rub_grade: Some(4.5),
        saliva_fastness_validated: false,
        evidence: Evidence::Measured,
        source: Source {
            reference: "local:conformance_exp4".to_string(),
            accessed_on: "2026-11-27".to_string(),
            note: "Synthetic Exp-4 qualification".to_string(),
        },
        method: "reactive fixation + soap boil wash".to_string(),
        conditioning: BTreeMap::new(),
        allow_estimates: false,
    };
    assert_eq!(
        verify_and_export_dye_program(&infant_prog, &machine, Some(&qual_ok), None)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );
    qual_ok.saliva_fastness_validated = true;
    assert!(verify_and_export_dye_program(&infant_prog, &machine, Some(&qual_ok), None).is_ok());

    // 7. Regime III dancer accumulator depletion fails closed
    let mut r3_machine = machine;
    r3_machine.accumulator = Some(DancerAccumulatorSpec {
        capacity_m: 1.0,
        initial_buffer_preload_m: 0.4,
        target_buffer_m: 0.5,
        dancer_kp_s_inv: 0.05, // deliberately weak gain
        min_fill_fraction: 0.1,
        max_fill_fraction: 0.8,
    });
    let r3_prog = YarnDyeProgram {
        schema: DYE_PROGRAM_SCHEMA.to_string(),
        name: "r3_overflow".to_string(),
        run_id: 0,
        yarn_id: "cotton-120tex".to_string(),
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        regime: DyeingRegime::Regime3InlineDancer,
        machine_sha256: r3_machine.canonical_sha256().unwrap(),
        qualification_sha256: None,
        startup_leader_m: 1.5,
        shutdown_trailer_m: 0.5,
        segments: vec![DyeSegment {
            s_start_m: 0.0,
            s_end_m: 10.0,
            target_speed_m_s: 0.08,
            doses: vec![0.2; 4],
            fiducial_mark: false,
        }],
        regime3_intake_timeline: Some(Regime3IntakeTimeline {
            events: vec![IntakeEvent {
                duration_s: 30.0,
                yarn_intake_speed_m_s: 0.40, // pulls much faster than max_line_speed -> empties dancer!
                kind: IntakeEventKind::KnitPass,
            }],
            dt_verify_s: 0.05,
        }),
        expected_delta_e00_p50: Some(3.0),
        expected_delta_e00_p95: Some(6.0),
        gamut_floor_delta_e00: Some(1.0),
        unreproducible_energy_fraction: Some(0.05),
        not_for_garment_wear: true,
        metadata: BTreeMap::new(),
    };
    r3_prog.validate().unwrap();
    let err_acc = verify_and_export_dye_program(&r3_prog, &r3_machine, None, None)
        .expect_err("dancer depletion must fail closed");
    assert_eq!(err_acc.code, RefusalCode::MaterialProcessHazard);
}

/// Minimal uncompressed RGBA PNG builder (pure Rust, zero external deps) for straight-alpha tests.
fn build_test_rgba_png(w: u32, h: u32, rgba: &[u8]) -> Vec<u8> {
    fn crc32(data: &[u8]) -> u32 {
        let mut crc = 0xFFFF_FFFFu32;
        for &b in data {
            crc ^= u32::from(b);
            for _ in 0..8 {
                let mask = (crc & 1).wrapping_neg();
                crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
            }
        }
        !crc
    }
    fn adler32(data: &[u8]) -> u32 {
        let (mut a, mut b) = (1u32, 0u32);
        for &d in data {
            a = (a + u32::from(d)) % 65521;
            b = (b + a) % 65521;
        }
        (b << 16) | a
    }
    fn chunk(kind: &[u8; 4], data: &[u8], out: &mut Vec<u8>) {
        out.extend_from_slice(&(data.len() as u32).to_be_bytes());
        let start = out.len();
        out.extend_from_slice(kind);
        out.extend_from_slice(data);
        let c = crc32(&out[start..]);
        out.extend_from_slice(&c.to_be_bytes());
    }
    let mut raw = Vec::new();
    let row_bytes = (w as usize) * 4;
    for y in 0..h as usize {
        raw.push(0); // filter None
        raw.extend_from_slice(&rgba[y * row_bytes..(y + 1) * row_bytes]);
    }
    let mut zlib = vec![0x78, 0x01, 0x01];
    let len = raw.len() as u16;
    zlib.extend_from_slice(&len.to_le_bytes());
    zlib.extend_from_slice(&(!len).to_le_bytes());
    zlib.extend_from_slice(&raw);
    zlib.extend_from_slice(&adler32(&raw).to_be_bytes());

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&w.to_be_bytes());
    ihdr.extend_from_slice(&h.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]); // 8-bit RGBA

    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    chunk(b"IHDR", &ihdr, &mut png);
    chunk(b"IDAT", &zlib, &mut png);
    chunk(b"IEND", &[], &mut png);
    png
}
