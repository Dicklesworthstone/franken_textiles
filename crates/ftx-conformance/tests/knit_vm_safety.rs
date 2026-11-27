//! Conformance tests for `FTX-K03` Pure-Kernel Leaf lifts (`lifted_tangle`, `lifted_procir`)
//! and adversarial V-bed Knitout VM safety invariants (`verify_knitout_program`).

use std::collections::BTreeMap;

use ftx_core::RefusalCode;
use ftx_pattern::knit::lifted_procir::golden_vectors::{
    PROCIR_KERNEL_GOLDEN_FNV64, PROCIR_KERNEL_GOLDEN_SHA256, build_afp_topology_spec,
    build_valid_afp_program, compute_golden_digests as procir_compute_golden_digests,
};
use ftx_pattern::knit::lifted_procir::{ProcessOp, SlotRef, ViolationKind, run_slot_rail_vm};
use ftx_pattern::knit::lifted_tangle::golden_vectors::{
    TANGLE_KERNEL_GOLDEN_FNV64, TANGLE_KERNEL_GOLDEN_SHA256, build_canonical_hopf_link,
    build_canonical_planar_tangle, compute_golden_digests as tangle_compute_golden_digests,
};
use ftx_pattern::knit::lifted_tangle::{CrossingSign, FtxDetMath, certify_topology_det};
use ftx_pattern::knit::{
    Bed, CamStitchSetting, CarrierId, CourseDir, GaugeSpec, KNIT_SCHEMA, KNITOUT_SCHEMA, KnitDoc,
    KnitSafetyContext, KnitoutHeader, KnitoutOp, KnitoutProgram, LoopKind, MachineProfile, Needle,
    ShortRowTurn, StitchNode, WearerAgeClass, YarnRun, verify_knitout_program,
};

fn base_knitout_program(profile: &MachineProfile, ops: Vec<KnitoutOp>) -> KnitoutProgram {
    KnitoutProgram {
        schema: KNITOUT_SCHEMA.to_string(),
        header: KnitoutHeader {
            machine: profile.id.clone(),
            gauge: format!("{}", profile.gauge_e),
            width: profile.needle_count,
            position: "Center".to_string(),
            carriers: vec![CarrierId("1".to_string())],
            extra: BTreeMap::new(),
        },
        ops,
    }
}

fn minimal_valid_knitout_ops() -> Vec<KnitoutOp> {
    let c1 = vec![CarrierId("1".to_string())];
    vec![
        KnitoutOp::In {
            carriers: c1.clone(),
        },
        KnitoutOp::Knit {
            dir: CourseDir::Plus,
            needle: Needle::new(Bed::Front, 10),
            carriers: c1.clone(),
        },
        KnitoutOp::Knit {
            dir: CourseDir::Minus,
            needle: Needle::new(Bed::Front, 10),
            carriers: c1.clone(),
        },
        KnitoutOp::Out { carriers: c1 },
    ]
}

fn base_knit_doc(age: WearerAgeClass, yarn_tex: Option<f64>) -> KnitDoc {
    KnitDoc {
        schema: KNIT_SCHEMA.to_string(),
        name: "vm_safety_test_doc".to_string(),
        wearer_age_class: age,
        gauge: GaugeSpec {
            wales_per_100mm: 22.0,
            courses_per_100mm: 30.0,
            needle_pitch_mm: 25.4 / 7.0,
            yarn_tex,
            thickness_mm: Some(2.0),
            relaxed_after_wash: true,
        },
        stitches: vec![StitchNode {
            id: 0,
            course: 0,
            wale: 10,
            bed: Bed::Front,
            kind: LoopKind::CastOn,
            parents: Vec::new(),
            carrier_ids: vec![CarrierId("1".to_string())],
            yarn_id: "merino-200tex".to_string(),
            cam_stitch_setting: CamStitchSetting {
                setting: 7.0,
                machine_family: "kniterate_7e".to_string(),
            },
            yarn_length_mm: Some(10.0),
        }],
        short_row_turns: Vec::new(),
        cable_groups: Vec::new(),
        yarn_runs: vec![YarnRun {
            run_id: 1,
            carrier_id: CarrierId("1".to_string()),
            yarn_id: "merino-200tex".to_string(),
            start_course: 0,
            end_course: 1,
            total_yarn_length_m: 5.0,
            cut_tail_mm: Some(12.0),
        }],
        has_hood_or_neck_drawstring: false,
        metadata: BTreeMap::new(),
    }
}

#[test]
fn lifted_tangle_golden_digests_match_pinned_frankensim() {
    let (fnv64, sha256) = tangle_compute_golden_digests::<FtxDetMath>();
    assert_eq!(fnv64, TANGLE_KERNEL_GOLDEN_FNV64);
    assert_eq!(sha256, TANGLE_KERNEL_GOLDEN_SHA256);
    assert_eq!(
        TANGLE_KERNEL_GOLDEN_SHA256,
        [
            0x31, 0x24, 0xc9, 0x56, 0x97, 0x88, 0x81, 0x75, 0xca, 0xcf, 0xf6, 0x7d, 0x41, 0x86,
            0x1d, 0xaf, 0xbd, 0x41, 0xe2, 0x68, 0x34, 0xb4, 0x68, 0xef, 0x67, 0x38, 0x98, 0x5d,
            0xb4, 0xd9, 0xb5, 0xf6,
        ]
    );
}

#[test]
fn lifted_procir_golden_digests_match_pinned_frankensim() {
    let (fnv64, sha256) = procir_compute_golden_digests();
    assert_eq!(fnv64, PROCIR_KERNEL_GOLDEN_FNV64);
    assert_eq!(sha256, PROCIR_KERNEL_GOLDEN_SHA256);
    assert_eq!(
        PROCIR_KERNEL_GOLDEN_SHA256,
        [
            0x06, 0x44, 0xc2, 0x59, 0xc5, 0xc0, 0xbe, 0x02, 0x22, 0xd9, 0x26, 0x02, 0x74, 0x19,
            0xa2, 0x98, 0x0a, 0x2b, 0xfa, 0x92, 0x36, 0x73, 0x36, 0xf3, 0x8a, 0xab, 0x82, 0xad,
            0x0e, 0xd3, 0xad, 0x35,
        ]
    );
}

#[test]
fn lifted_tangle_hopf_linking_and_crossing_flip_detection() {
    let hopf = build_canonical_hopf_link::<FtxDetMath>();
    let cert = certify_topology_det(&hopf).expect("hopf link topology certificate");
    assert!(cert.isotopy_valid);
    assert_eq!(cert.pairwise_linking, vec![(1, 2, 1)]);
    assert!(cert.max_linking_residual < 1e-12);

    // Flipping the prescribed crossing sign must fail isotopy verification.
    let mut flipped = build_canonical_planar_tangle::<FtxDetMath>();
    assert!(
        certify_topology_det(&flipped)
            .expect("baseline planar tangle")
            .isotopy_valid
    );
    flipped.crossings[0].expected_sign = CrossingSign::Negative;
    let flipped_cert = certify_topology_det(&flipped).expect("flipped planar tangle");
    assert!(!flipped_cert.isotopy_valid);
    assert_eq!(flipped_cert.failed_crossings, vec![0]);
}

#[test]
fn lifted_procir_discrete_vm_accepts_valid_and_rejects_safety_violations() {
    let spec = build_afp_topology_spec();
    let valid_prog = build_valid_afp_program(&spec);
    let valid_receipt = run_slot_rail_vm(&spec, &valid_prog);
    assert!(valid_receipt.feasible);
    assert!(valid_receipt.violations.is_empty());

    // 1. Rail collision (Head 2 approaches within min_clearance_slots=10 of Head 1 at 20).
    let mut collision_prog = valid_prog.clone();
    collision_prog.ops.insert(
        10,
        ProcessOp::MoveActuator {
            actuator_id: 2,
            target_position: 25,
        },
    );
    let collision_receipt = run_slot_rail_vm(&spec, &collision_prog);
    assert!(!collision_receipt.feasible);
    assert!(
        collision_receipt
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::RailCollision)
    );

    // 2. Slot occupancy overflow (Bank 0 cap is 2; insert 3 tokens into slot (0, 10)).
    let mut overflow_prog = valid_prog.clone();
    overflow_prog.ops.insert(
        4,
        ProcessOp::InsertToken {
            actuator_id: 1,
            slot: SlotRef {
                bank_id: 0,
                slot: 10,
            },
            token_id: 102,
        },
    );
    overflow_prog.ops.insert(
        5,
        ProcessOp::InsertToken {
            actuator_id: 1,
            slot: SlotRef {
                bank_id: 0,
                slot: 10,
            },
            token_id: 103,
        },
    );
    let overflow_receipt = run_slot_rail_vm(&spec, &overflow_prog);
    assert!(!overflow_receipt.feasible);
    assert!(
        overflow_receipt
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SlotOccupancyOverflow)
    );

    // 3. Tampered machine_spec_sha256 -> SpecHashMismatch.
    let mut tampered_prog = valid_prog;
    tampered_prog.machine_spec_sha256[0] ^= 0xff;
    let tampered_receipt = run_slot_rail_vm(&spec, &tampered_prog);
    assert!(!tampered_receipt.feasible);
    assert!(
        tampered_receipt
            .violations
            .iter()
            .any(|v| v.kind == ViolationKind::SpecHashMismatch)
    );
}

#[test]
fn vbed_vm_enforces_slider_ban_carrier_lifecycle_spacing_and_racking() {
    let kniterate = MachineProfile::kniterate_7e();
    let adult_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::Adult,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: None,
        speed_fraction: None,
        takedown_fraction: None,
    };
    let c1 = vec![CarrierId("1".to_string())];

    // 1. Slider-enabled profile permits Xfer to FrontSlider, but forbids Knit/Tuck/Split on sliders.
    let mut slider_prof = kniterate.clone();
    slider_prof.has_sliders = true;

    let xfer_slider_ok = base_knitout_program(
        &slider_prof,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::BackSlider, 10),
            },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::BackSlider, 10),
                to: Needle::new(Bed::Front, 10),
            },
            KnitoutOp::Out {
                carriers: c1.clone(),
            },
        ],
    );
    assert!(verify_knitout_program(&xfer_slider_ok, &slider_prof, &adult_ctx).is_ok());

    for bad_slider_op in [
        KnitoutOp::Knit {
            dir: CourseDir::Plus,
            needle: Needle::new(Bed::FrontSlider, 10),
            carriers: c1.clone(),
        },
        KnitoutOp::Tuck {
            dir: CourseDir::Plus,
            needle: Needle::new(Bed::BackSlider, 10),
            carriers: c1.clone(),
        },
        KnitoutOp::Split {
            dir: CourseDir::Plus,
            from: Needle::new(Bed::FrontSlider, 10),
            to: Needle::new(Bed::Back, 10),
            carriers: c1.clone(),
        },
    ] {
        let prog = base_knitout_program(
            &slider_prof,
            vec![
                KnitoutOp::In {
                    carriers: c1.clone(),
                },
                bad_slider_op,
                KnitoutOp::Out {
                    carriers: c1.clone(),
                },
            ],
        );
        assert_eq!(
            verify_knitout_program(&prog, &slider_prof, &adult_ctx)
                .unwrap_err()
                .code,
            RefusalCode::MachineStateInfeasible
        );
    }

    // 2. Carrier lifecycle: unparked carrier at EOF, double `in`, knit before `in`, `inhook` on Kniterate.
    let unparked = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
        ],
    );
    assert_eq!(
        verify_knitout_program(&unparked, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );

    let double_in = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Out {
                carriers: c1.clone(),
            },
        ],
    );
    assert_eq!(
        verify_knitout_program(&double_in, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );

    let knit_before_in = base_knitout_program(
        &kniterate,
        vec![KnitoutOp::Knit {
            dir: CourseDir::Plus,
            needle: Needle::new(Bed::Front, 10),
            carriers: c1.clone(),
        }],
    );
    assert_eq!(
        verify_knitout_program(&knit_before_in, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );

    let inhook_forbidden = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::InHook {
                carriers: c1.clone(),
            },
            KnitoutOp::ReleaseHook {
                carriers: c1.clone(),
            },
            KnitoutOp::OutHook {
                carriers: c1.clone(),
            },
        ],
    );
    assert_eq!(
        verify_knitout_program(&inhook_forbidden, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );

    // 3. Carrier spacing below Kniterate min_carrier_spacing (2).
    let tight_spacing = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::CarrierSpacing { needles: 1 },
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Out {
                carriers: c1.clone(),
            },
        ],
    );
    assert_eq!(
        verify_knitout_program(&tight_spacing, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );

    // 4. Racking bounds & non-integer quarter-pitch transfer alignment.
    let over_rack = base_knitout_program(
        &kniterate,
        vec![KnitoutOp::Rack {
            quarter_pitches: kniterate.max_racking_quarter_pitches + 4,
        }],
    );
    assert_eq!(
        verify_knitout_program(&over_rack, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );

    let half_pitch_xfer = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Rack { quarter_pitches: 2 },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::Back, 10),
            },
            KnitoutOp::Out { carriers: c1 },
        ],
    );
    assert_eq!(
        verify_knitout_program(&half_pitch_xfer, &kniterate, &adult_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MachineStateInfeasible
    );
}

#[test]
fn vbed_vm_enforces_tex_range_child_float_infant_safety_and_swatch_caps() {
    let kniterate = MachineProfile::kniterate_7e();
    let base_prog = base_knitout_program(&kniterate, minimal_valid_knitout_ops());

    // 1. Out-of-range yarn tex refused (recommended_tex_range on Kniterate 7E is [60, 450]).
    let heavy_doc = base_knit_doc(WearerAgeClass::Adult, Some(800.0));
    let heavy_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::Adult,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&heavy_doc),
        speed_fraction: None,
        takedown_fraction: None,
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &heavy_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    // 2. Child float limit (> 12 mm).
    let c1 = vec![CarrierId("1".to_string())];
    let long_float_prog = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 16), // 5 skipped needles * 4 mm = 20 mm > 12 mm
                carriers: c1.clone(),
            },
            KnitoutOp::Out {
                carriers: c1.clone(),
            },
        ],
    );
    let child_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::Child3To14,
        blocked_wale_pitch_mm: 4.0,
        source_doc: None,
        speed_fraction: None,
        takedown_fraction: None,
    };
    assert_eq!(
        verify_knitout_program(&long_float_prog, &kniterate, &child_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    // 3. InfantUnder3 cut-tail (> 20 mm), unanchored short-row eyelet, and EN 14682 drawstring.
    let mut bad_tail_doc = base_knit_doc(WearerAgeClass::InfantUnder3, Some(200.0));
    bad_tail_doc.yarn_runs[0].cut_tail_mm = Some(25.0);
    let infant_tail_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::InfantUnder3,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&bad_tail_doc),
        speed_fraction: None,
        takedown_fraction: None,
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &infant_tail_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let mut unanchored_sr_doc = base_knit_doc(WearerAgeClass::InfantUnder3, Some(200.0));
    unanchored_sr_doc.short_row_turns.push(ShortRowTurn {
        course: 2,
        turn_wale: 15,
        dir_after: CourseDir::Minus,
        anchored: false,
    });
    let infant_sr_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::InfantUnder3,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&unanchored_sr_doc),
        speed_fraction: None,
        takedown_fraction: None,
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &infant_sr_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let mut drawstring_doc = base_knit_doc(WearerAgeClass::InfantUnder3, Some(200.0));
    drawstring_doc.has_hood_or_neck_drawstring = true;
    let infant_drawstring_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::InfantUnder3,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&drawstring_doc),
        speed_fraction: None,
        takedown_fraction: None,
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &infant_drawstring_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    // 4. CalibrationSwatchOnly caps: speed > 0.50, takedown > 0.60, yarn > 40 m, needles > 80,
    //    courses > 200, rack != 0, and xfer forbidden.
    let swatch_doc_ok = base_knit_doc(WearerAgeClass::CalibrationSwatchOnly, Some(200.0));
    let fast_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&swatch_doc_ok),
        speed_fraction: Some(0.55),
        takedown_fraction: Some(0.50),
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &fast_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let high_td_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&swatch_doc_ok),
        speed_fraction: Some(0.45),
        takedown_fraction: Some(0.65),
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &high_td_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let mut long_yarn_doc = base_knit_doc(WearerAgeClass::CalibrationSwatchOnly, Some(200.0));
    long_yarn_doc.yarn_runs[0].total_yarn_length_m = 45.0;
    let long_yarn_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: Some(&long_yarn_doc),
        speed_fraction: Some(0.45),
        takedown_fraction: Some(0.50),
    };
    assert_eq!(
        verify_knitout_program(&base_prog, &kniterate, &long_yarn_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let swatch_ok_ctx = KnitSafetyContext {
        wearer_age_class: WearerAgeClass::CalibrationSwatchOnly,
        blocked_wale_pitch_mm: kniterate.needle_pitch_mm,
        source_doc: None,
        speed_fraction: Some(0.45),
        takedown_fraction: Some(0.50),
    };
    let wide_swatch = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Minus,
                needle: Needle::new(Bed::Front, 95), // span 86 > MAX_NEEDLES (80)
                carriers: c1.clone(),
            },
            KnitoutOp::Out {
                carriers: c1.clone(),
            },
        ],
    );
    assert_eq!(
        verify_knitout_program(&wide_swatch, &kniterate, &swatch_ok_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let mut many_courses_ops = vec![KnitoutOp::In {
        carriers: c1.clone(),
    }];
    for c in 0..201 {
        let dir = if c % 2 == 0 {
            CourseDir::Plus
        } else {
            CourseDir::Minus
        };
        many_courses_ops.push(KnitoutOp::Knit {
            dir,
            needle: Needle::new(Bed::Front, 10),
            carriers: c1.clone(),
        });
    }
    many_courses_ops.push(KnitoutOp::Out {
        carriers: c1.clone(),
    });
    let many_courses_swatch = base_knitout_program(&kniterate, many_courses_ops);
    assert_eq!(
        verify_knitout_program(&many_courses_swatch, &kniterate, &swatch_ok_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let rack_swatch =
        base_knitout_program(&kniterate, vec![KnitoutOp::Rack { quarter_pitches: 4 }]);
    assert_eq!(
        verify_knitout_program(&rack_swatch, &kniterate, &swatch_ok_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );

    let xfer_swatch = base_knitout_program(
        &kniterate,
        vec![
            KnitoutOp::In {
                carriers: c1.clone(),
            },
            KnitoutOp::Knit {
                dir: CourseDir::Plus,
                needle: Needle::new(Bed::Front, 10),
                carriers: c1.clone(),
            },
            KnitoutOp::Xfer {
                from: Needle::new(Bed::Front, 10),
                to: Needle::new(Bed::Back, 10),
            },
            KnitoutOp::Out { carriers: c1 },
        ],
    );
    assert_eq!(
        verify_knitout_program(&xfer_swatch, &kniterate, &swatch_ok_ctx)
            .unwrap_err()
            .code,
        RefusalCode::MaterialProcessHazard
    );
}
