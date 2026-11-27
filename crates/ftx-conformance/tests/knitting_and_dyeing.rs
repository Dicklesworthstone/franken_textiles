//! Cross-crate conformance, safety, physics KAT, and determinism tests for computational knitting
//! and programmed continuous length-wise yarn dyeing (`FTX-K01`..`FTX-K19`).
//!
//! Explicitly separates the Four Claims:
//! - Parity/format contracts (Knitout v2, Kniterate `.kc`, Brother AYAB 1-bit PNG, vector CYC SVG/PDF),
//! - Physics KATs (Poincloux et al. 2018 Rung-3 elastica, exact Kubelka-Munk / Sharma 2005 CIEDE2000,
//!   conserved-mass capillary transport + VJP, Sobol-CVaR Bertsekas L-BFGS),
//! - Honesty & safety invariants (child/infant EN 14682 / float / eyelet / tail / fastness gates,
//!   Tier-B proprietary target refusal, `hardware_execution_authorized: false`, `fit_claimed: false`),
//! - Bit-exact determinism across repeated executions.

use std::collections::BTreeMap;

use ftx_core::det;
use ftx_core::receipt::canonical_json_sha256;
use ftx_core::{EaseProfile, EaseShellMesh, KnitShellRegion, RefusalCode, TargetRaster, Vec3};
use ftx_drape::dye::{
    CapillaryTransportConfig, CieLab, CieXyz, DriftParameters, DyeInverseConfig, DyeOpticsModel,
    KnitStitchMap, ciede2000_exact, ciede2000_sq_smooth_grad, coherent_open_loop_horizons,
    forward_capillary_transport, linear_xyz_to_lab, optimize_yarn_dye_program,
    rank_five_regime_pareto, srgb_u8_to_xyz_d65, vjp_capillary_transport,
};
use ftx_drape::knit::{
    AsymmetricKnitBending, CoupledKnitMembrane, KnitHysteresisBank, LocalAuditPatchInput,
    PoinclouxParams, RveHomogenizedKnitCard, assess_knit_shell_state, audit_local_knit_patch,
    calibrate_knit_prior_to_uniaxial_strips, cayley_hamilton_inv_sqrt_2x2,
    poincloux_uniaxial_curve,
};
use ftx_export::dye::verify_and_export_dye_program;
use ftx_export::knit::{
    check_hand_knit_pdf, check_hand_knit_svg, export_brother_ayab, export_calibration_swatch_kit,
    export_hand_knit_document, export_kniterate_kc, export_verified_knitout,
    refuse_proprietary_binary_target,
};
use ftx_pattern::dye::{
    DancerAccumulatorSpec, DyeMachineProfile, DyeSegment, DyeingRegime, IntakeEvent,
    Regime3IntakeTimeline, YarnDyeProgram, YarnDyeingQualification,
};
use ftx_pattern::knit::{
    CarrierId, GaugeSpec, KnitSwatchQualification, KnitVmContext, KnitoutOp, KnitoutProgram,
    MachineProfile, ParametricPanelSpec, RaglanPulloverSpec, WearerAgeClass, compile_knit_doc,
    draft_autoknit_from_shell, draft_calibration_swatch, draft_seamless_raglan_tube,
    draft_shaped_panel, parse_knitout, verify_knitout_program,
};
use ftx_pattern::materials::Evidence;
use ftx_trace::raster::load_target_raster_with_limit;

fn default_gauge() -> GaugeSpec {
    GaugeSpec {
        wales_per_100mm: 22.0,
        courses_per_100mm: 30.0,
        yarn_tex: Some(200.0),
        measured_after_wash_block: false,
    }
}

fn synthetic_swatch_qual(profile: &MachineProfile, yarn_id: &str) -> KnitSwatchQualification {
    KnitSwatchQualification {
        schema: "ftx-knit-swatch-qual/1".to_string(),
        machine_profile_id: profile.id.clone(),
        yarn_id: yarn_id.to_string(),
        yarn_lot_id: "lot-test-001".to_string(),
        washed_and_blocked: true,
        measured_wales_per_100mm: 22.0,
        measured_courses_per_100mm: 30.0,
        measured_loop_length_mm: 5.5,
        measured_drift_eps: Some(0.002),
        measured_drift_sigma_eta: Some(0.015),
        recorded_on: "2026-11-27".to_string(),
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
fn knitting_end_to_end_swatch_panel_raglan_and_tier_a_b_exports() {
    let kniterate = MachineProfile::kniterate_7e();
    let ayab = MachineProfile::brother_kh930_ayab();
    let gauge = default_gauge();
    let qual = synthetic_swatch_qual(&kniterate, "merino-200tex");

    // 1. Calibration swatch kit (Exp-3) passes without prior qualification
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
    assert!(!kit.knitout.cert.fit_claimed);
    let reparsed = parse_knitout(&kit.knitout.knitout_text).unwrap();
    assert_eq!(reparsed.to_canonical_string(), kit.knitout.knitout_text);

    // 2. Measured shaped panel for toddler refuses without Exp-3 swatch qualification on garment export
    let panel_spec = ParametricPanelSpec {
        name: "toddler_sweater_front".to_string(),
        wearer_age_class: WearerAgeClass::Toddler12_36m,
        hem_width_mm: 240.0,
        chest_width_mm: 270.0,
        length_mm: 290.0,
        armhole_depth_mm: 90.0,
        armhole_inset_total_mm: 36.0,
        neck_width_mm: 100.0,
        neck_depth_mm: 22.0,
        rib_hem_courses: 6,
        cam_setting: 7.0,
        yarn_id: "merino-200tex".to_string(),
        carrier_id: CarrierId("1".to_string()),
        has_hood_or_neck_drawstring: false,
    };
    let panel_doc = draft_shaped_panel(&panel_spec, &gauge, &kniterate).unwrap();
    let err_unqual = compile_knit_doc(&panel_doc, &kniterate, None, None, None).unwrap_err();
    assert_eq!(err_unqual.code, RefusalCode::UnqualifiedProcess);

    // With Exp-3 qualification, compiles cleanly + emits Kniterate .kc + hand-knit vector SVG/PDF!
    let compiled = compile_knit_doc(&panel_doc, &kniterate, Some(&qual), None, None).unwrap();
    let ko = export_verified_knitout(&compiled).unwrap();
    let kc = export_kniterate_kc(&compiled, &kniterate).unwrap();
    assert!(kc.kc_text.contains("GRIPPER_IN 1"));
    assert!(!kc.cert.hardware_execution_authorized);

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
    assert!(!ayab_bundle.checklist.hardware_execution_authorized);

    // 4. EN 14682 child drawstring ban refused
    let mut bad_panel = panel_spec.clone();
    bad_panel.has_hood_or_neck_drawstring = true;
    let bad_doc = draft_shaped_panel(&bad_panel, &gauge, &kniterate).unwrap();
    let err_drawstring = compile_knit_doc(&bad_doc, &kniterate, Some(&qual), None, None)
        .expect_err("EN 14682 must refuse child drawstring");
    assert_eq!(err_drawstring.code, RefusalCode::MaterialProcessHazard);

    // 5. Tier-B proprietary target fails closed
    let shima = MachineProfile::shima_seiki_12e_handoff();
    let err_tier_b = refuse_proprietary_binary_target(&shima, "DAT").unwrap_err();
    assert_eq!(err_tier_b.code, RefusalCode::OpaqueProprietaryTarget);

    // 6. Seamless tubular raglan pullover compiles and verifies bit-deterministically
    let raglan = RaglanPulloverSpec {
        name: "toddler_raglan".to_string(),
        wearer_age_class: WearerAgeClass::Toddler12_36m,
        neck_girth_mm: 260.0,
        chest_girth_mm: 520.0,
        yoke_depth_mm: 120.0,
        body_length_below_arm_mm: 180.0,
        collar_rib_courses: 6,
        hem_rib_courses: 6,
        cam_setting: 7.0,
        yarn_id: "merino-200tex".to_string(),
        carrier_id: CarrierId("1".to_string()),
        has_hood_or_neck_drawstring: false,
    };
    let rag_doc = draft_seamless_raglan_tube(&raglan, &gauge, &kniterate).unwrap();
    let rag1 = compile_knit_doc(&rag_doc, &kniterate, Some(&qual), None, None).unwrap();
    let rag2 = compile_knit_doc(&rag_doc, &kniterate, Some(&qual), None, None).unwrap();
    assert_eq!(rag1.cert.program_sha256, rag2.cert.program_sha256);
    assert_eq!(ko.knitout_sha256, compiled.cert.program_sha256);

    // 7. VM refuses slider ops on Kniterate 7E (no sliders)
    let mut slider_prog = KnitoutProgram::new(&kniterate, vec![CarrierId("1".to_string())]);
    slider_prog.ops.push(KnitoutOp::In {
        carriers: vec![CarrierId("1".to_string())],
    });
    slider_prog.ops.push(KnitoutOp::Xfer {
        from: ftx_pattern::knit::NeedleRef {
            bed: ftx_pattern::knit::Bed::Front,
            slider: false,
            index: 1,
        },
        to: ftx_pattern::knit::NeedleRef {
            bed: ftx_pattern::knit::Bed::Front,
            slider: true,
            index: 1,
        },
    });
    let vm_ctx = KnitVmContext {
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        effective_wale_pitch_mm: None,
        speed_fraction: Some(0.5),
        takedown_fraction: Some(0.5),
        doc: None,
        swatch_qual: None,
        drape_uncertainty: None,
    };
    let err_slider = verify_knitout_program(&slider_prog, &kniterate, &vm_ctx).unwrap_err();
    assert_eq!(err_slider.code, RefusalCode::MachineStateInfeasible);
}

#[test]
fn autoknit_pathway2_drafts_harmonic_frustum_shell() {
    let rings = 6usize;
    let segs = 16usize;
    let mut vertices = Vec::new();
    let mut triangles = Vec::new();
    for r in 0..=rings {
        let z = 0.05 + 0.25 * (r as f64 / rings as f64);
        let rad = 0.085 - 0.015 * (r as f64 / rings as f64);
        for s in 0..segs {
            let theta = 2.0 * std::f64::consts::PI * (s as f64 / segs as f64);
            vertices.push(Vec3::new(rad * det::cos(theta), rad * det::sin(theta), z));
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
        region: KnitShellRegion::TorsoTube,
        vertices,
        triangles,
        boundary_rings: vec![
            ftx_core::BoundaryRing {
                name: "hem".to_string(),
                vertex_indices: start_ring,
                perimeter_m: 2.0 * std::f64::consts::PI * 0.085,
            },
            ftx_core::BoundaryRing {
                name: "neck".to_string(),
                vertex_indices: end_ring,
                perimeter_m: 2.0 * std::f64::consts::PI * 0.070,
            },
        ],
        ease_applied: EaseProfile::default(),
    };
    let profile = MachineProfile::kniterate_7e();
    let res = draft_autoknit_from_shell(
        &shell,
        "frustum_tube",
        WearerAgeClass::Toddler12_36m,
        &default_gauge(),
        &profile,
        "merino-200tex",
        CarrierId("1".to_string()),
        7.0,
    )
    .unwrap();
    assert!(res.harmonic_min_cot_weight > -1e-6);
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
        mem.evaluate([mem.e_w_max + 0.05, 0.0, 0.0])
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
    let hyst = KnitHysteresisBank::reference_jersey_estimate();
    let s0 = hyst.zero_state();
    let (sig1, _d1, next1) = hyst.evaluate_trial(&s0, [0.15, 0.05, 0.0]);
    let (sig2, _d2, _) = hyst.evaluate_trial(&s0, [0.15, 0.05, 0.0]);
    assert_eq!(sig1, sig2);
    assert!(next1.play_centers != s0.play_centers);

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
    assert_eq!(assess.receipt.flags.len(), 5);
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

    // 3. Coherent horizons obey 4x flatbed vs circular penalty
    let drift = DriftParameters {
        systematic_eps: 0.002,
        stochastic_sigma_eta: 0.015,
    };
    let (h_circ, h_flat) = coherent_open_loop_horizons(drift);
    assert!(h_circ > h_flat * 3);

    // 4. Full 4-stage Sobol-CVaR Bertsekas L-BFGS optimization + bit-exact repeatability
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
        max_iters: 15,
        skin_underlayer_xyz: srgb_u8_to_xyz_d65([215, 180, 155]),
        ciede_eps_c: 1e-4,
        drift,
    };
    let res1 =
        optimize_yarn_dye_program(&target, &stitch_map, &machine, &optics, &tr_cfg, &inv_cfg)
            .unwrap();
    let res2 =
        optimize_yarn_dye_program(&target, &stitch_map, &machine, &optics, &tr_cfg, &inv_cfg)
            .unwrap();
    assert_eq!(res1.predicted_stitch_srgb, res2.predicted_stitch_srgb);
    assert_eq!(res1.delta_e00_p50.to_bits(), res2.delta_e00_p50.to_bits());

    let pareto = rank_five_regime_pareto(&target, &stitch_map, &machine, drift, 4);
    assert_eq!(pareto.len(), 5);

    // 5. Swatch verification & export produces CSV + G-code + sealed cert
    let bundle = verify_and_export_dye_program(&res1.program, &machine, None, None).unwrap();
    assert!(!bundle.cert.hardware_execution_authorized);
    assert!(!bundle.cert.physical_dyeing_validated);
    assert!(bundle.controller_gcode.contains("G1 E"));

    // 6. Infant wearable refuses without saliva fastness receipt, passes with ISO 105-X12 >= 4.0
    let mut infant_prog = res1.program.clone();
    infant_prog.wearer_age_class = WearerAgeClass::Infant0_12m;
    assert_eq!(
        verify_and_export_dye_program(&infant_prog, &machine, None, None)
            .unwrap_err()
            .code,
        RefusalCode::UnqualifiedProcess
    );
    let qual_ok = YarnDyeingQualification {
        schema: "ftx-dye-qual/1".to_string(),
        dye_machine_id: machine.id.clone(),
        yarn_id: "cotton-120tex".to_string(),
        dye_lot_id: "lot-d1".to_string(),
        wash_off_complete: true,
        iso_105_c06_wash_fastness_grade: Some(4.5),
        iso_105_x12_rub_fastness_grade: Some(4.5),
        saliva_fastness_tested_and_passed: true,
        recorded_on: "2026-11-27".to_string(),
    };
    assert!(verify_and_export_dye_program(&infant_prog, &machine, Some(&qual_ok), None).is_ok());

    // 7. Regime III dancer accumulator overflow fails closed
    let r3_prog = YarnDyeProgram {
        schema: "ftx-dye-program/1".to_string(),
        name: "r3_overflow".to_string(),
        yarn_id: "cotton-120tex".to_string(),
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        regime: DyeingRegime::Regime3OnlineTandem,
        primary_names: vec!["C".into(), "M".into(), "Y".into(), "K".into()],
        leader_length_m: 1.5,
        trailer_length_m: 1.5,
        segments: vec![DyeSegment {
            s_start_m: 0.0,
            s_end_m: 10.0,
            concentrations_mol_m3: vec![0.2; 4],
            target_speed_m_s: 0.15,
            dwell_s: 0.05,
        }],
        predicted_ciede2000_p50: Some(3.0),
        predicted_ciede2000_p95: Some(6.0),
        gamut_floor_delta_e00: Some(1.0),
        metadata: BTreeMap::new(),
    };
    r3_prog.validate().unwrap();
    let mut r3_machine = machine;
    r3_machine.dancer_accumulator = Some(DancerAccumulatorSpec {
        min_buffer_m: 0.1,
        max_buffer_m: 0.8,
        initial_buffer_m: 0.4,
        kp_s_inv: 0.05, // deliberately weak gain
        v_min_m_s: 0.02,
        v_max_m_s: 0.20,
    });
    let timeline = Regime3IntakeTimeline {
        events: vec![IntakeEvent {
            t_start_s: 0.0,
            t_end_s: 30.0,
            intake_rate_m_s: 0.40, // pulls much faster than v_max -> empties dancer!
            note: "rapid burst".to_string(),
        }],
    };
    let err_acc = verify_and_export_dye_program(&r3_prog, &r3_machine, None, Some(&timeline))
        .expect_err("dancer depletion must fail closed");
    assert_eq!(err_acc.code, RefusalCode::MachineStateInfeasible);
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
