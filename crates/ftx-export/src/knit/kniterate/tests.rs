use super::*;
use ftx_pattern::knit::{
    KNITOUT_SCHEMA, KnitoutHeader, compile_knit_doc, draft_calibration_swatch,
};

const KEEP: &str = include_str!("../../../tests/fixtures/kniterate/position-keep.kc");
const XFER: &str = include_str!("../../../tests/fixtures/kniterate/xfer-style.kc");
const STALE_SPLIT: &str = include_str!("../../../tests/fixtures/kniterate/split.kc");

#[test]
fn reads_frozen_upstream_files_without_running_the_emitter() {
    let keep = read_kniterate_kc(KEEP).unwrap();
    assert_eq!(
        (
            keep.pass_count,
            keep.tuck_count,
            keep.knit_count,
            keep.drop_count
        ),
        (5, 7, 14, 7)
    );
    assert_eq!(keep.needle_operation_count, 28);
    let xfer = read_kniterate_kc(XFER).unwrap();
    assert_eq!(
        (
            xfer.pass_count,
            xfer.tuck_count,
            xfer.knit_count,
            xfer.transfer_count,
            xfer.drop_count
        ),
        (47, 8, 72, 68, 8)
    );
    assert_eq!(xfer.needle_operation_count, 156);
    // Upstream commit b19ae3d fixed negative split directions in the converter,
    // but did not regenerate this older fixture. Keep the original bytes and
    // require a rejection instead of teaching the reader to accept the defect.
    let refusal = read_kniterate_kc(STALE_SPLIT).unwrap_err();
    assert!(refusal.message.contains("receiving cam direction"));
}

#[test]
fn rejects_upstream_raster_and_motion_corruption() {
    for text in [
        KEEP.replacen("FRNT:", "FRNT:-", 1),
        KEEP.replacen("STIF:5", "STIF:G", 1),
        KEEP.replacen(">> Tu-Tu 6", "<< Tu-Tu 6", 1),
        KEEP.replacen("RACK:0", "RACK:5", 1),
        KEEP.replacen("/....6", "/....5", 1),
        XFER.replacen("<< Tr-Rl", "<< Tr-Rr", 1),
        "HEADER MACHINE=KNITERATE_7E\nEND\n".to_string(),
    ] {
        assert_eq!(
            read_kniterate_kc(&text).unwrap_err().code,
            RefusalCode::VerificationMismatch
        );
    }
}

#[test]
fn exports_actual_passes_calibrated_dials_and_executable_manual_stop() {
    let profile = MachineProfile::kniterate_7e();
    let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 12, 4, &[6.0, 10.0]).unwrap();
    let bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
    let kc = export_kniterate_kc(&bundle, &profile).unwrap();
    assert!(kc.kc_text.starts_with("HOME\nRACK:0\n"));
    assert!(kc.kc_text.contains("FRNT:"));
    assert!(kc.kc_text.contains("STIF:AAAA"));
    assert!(kc.kc_text.contains("\nPause\n"));
    assert_eq!(kc.verification.operator_pause_count, 1);
    assert_eq!(kc.verification.needle_operation_count, doc.stitches.len());
    assert_eq!(kc.verification.kc_sha256, kc.kc_sha256);
    assert_eq!(kc.verification.program_sha256, bundle.cert.program_sha256);
    assert!(!kc.cert.hardware_execution_authorized);
    assert!(!kc.verification.hardware_execution_authorized);
    verify_kniterate_kc_roundtrip(&kc.kc_text, &bundle, &profile).unwrap();
    assert_eq!(
        kc.kc_text,
        export_kniterate_kc(&bundle, &profile).unwrap().kc_text
    );
    for changed in [
        kc.kc_text
            .replacen("STIF:6", "STIF:7", 1)
            .replace("6666", "7777"),
        kc.kc_text.replacen(" 150 100", " 151 100", 1),
        kc.kc_text.replacen(" 150 100", " 150 101", 1),
        kc.kc_text.replace("\nPause\n", "\n// Pause\n"),
    ] {
        assert_ne!(changed, kc.kc_text);
        assert_eq!(
            verify_kniterate_kc_roundtrip(&changed, &bundle, &profile)
                .unwrap_err()
                .code,
            RefusalCode::VerificationMismatch
        );
    }
}

fn program(ops: Vec<KnitoutOp>) -> KnitoutProgram {
    KnitoutProgram {
        schema: KNITOUT_SCHEMA.into(),
        header: KnitoutHeader {
            machine: "kniterate-7e".into(),
            gauge: "7".into(),
            width: 252,
            position: "Center".into(),
            carriers: vec![CarrierId("1".into()), CarrierId("2".into())],
            extra: BTreeMap::new(),
        },
        ops,
    }
}

#[test]
fn transfers_splits_racking_cams_and_two_carriers_preserve_source_actions() {
    let c1 = vec![CarrierId("1".into())];
    let c2 = vec![CarrierId("2".into())];
    let f = |n| Needle::new(Bed::Front, n);
    let b = |n| Needle::new(Bed::Back, n);
    let p = program(vec![
        KnitoutOp::In {
            carriers: c1.clone(),
        },
        KnitoutOp::Stitch {
            leading: 60,
            trailing: 60,
        },
        KnitoutOp::Tuck {
            dir: CourseDir::Plus,
            needle: f(100),
            carriers: c1.clone(),
        },
        KnitoutOp::Tuck {
            dir: CourseDir::Plus,
            needle: f(101),
            carriers: c1.clone(),
        },
        KnitoutOp::Knit {
            dir: CourseDir::Minus,
            needle: f(101),
            carriers: c1.clone(),
        },
        KnitoutOp::Knit {
            dir: CourseDir::Minus,
            needle: f(100),
            carriers: c1.clone(),
        },
        KnitoutOp::Xfer {
            from: f(100),
            to: b(100),
        },
        KnitoutOp::Rack { quarter_pitches: 4 },
        KnitoutOp::Xfer {
            from: b(100),
            to: f(101),
        },
        KnitoutOp::Rack { quarter_pitches: 0 },
        KnitoutOp::Split {
            dir: CourseDir::Plus,
            from: f(101),
            to: b(101),
            carriers: c1.clone(),
        },
        KnitoutOp::Stitch {
            leading: 70,
            trailing: 70,
        },
        KnitoutOp::Knit {
            dir: CourseDir::Minus,
            needle: f(101),
            carriers: c1.clone(),
        },
        KnitoutOp::Split {
            dir: CourseDir::Minus,
            from: b(101),
            to: f(101),
            carriers: c1.clone(),
        },
        KnitoutOp::In {
            carriers: c2.clone(),
        },
        KnitoutOp::Tuck {
            dir: CourseDir::Plus,
            needle: f(110),
            carriers: c2.clone(),
        },
        KnitoutOp::Knit {
            dir: CourseDir::Minus,
            needle: f(110),
            carriers: c2.clone(),
        },
        KnitoutOp::Knit {
            dir: CourseDir::Plus,
            needle: f(101),
            carriers: c1.clone(),
        },
        KnitoutOp::Pause {
            note: Some("Secure both tails".into()),
        },
        KnitoutOp::Out { carriers: c1 },
        KnitoutOp::Out { carriers: c2 },
        KnitoutOp::Drop { needle: f(101) },
        KnitoutOp::Drop { needle: f(110) },
    ]);
    let entries = schedule(&p, &MachineProfile::kniterate_7e()).unwrap();
    let kc = render(&entries).unwrap();
    let (report, takedown) =
        check_readback(&kc, &p, &entries, &MachineProfile::kniterate_7e()).unwrap();
    assert!(!takedown.physical_takedown_calibrated);
    assert!(takedown.max_held_advance_units <= takedown.limit_units);
    assert_eq!(report.transfer_count, 2);
    assert_eq!(report.split_count, 2);
    assert_eq!(report.operator_pause_count, 1);
    assert_eq!(report.drop_count, 2);
    assert_eq!(report.max_abs_rack, 1);
    for changed in [
        kc.replace("// Operator: Secure both tails\n", ""),
        kc.replace("Secure both tails", "Cut both tails now"),
    ] {
        let decoded = readback::decode(&changed).unwrap();
        assert!(readback::compare_program(&decoded, &p).is_err());
        assert!(readback::compare_schedule(&decoded, &entries).is_err());
    }
}

#[test]
fn miss_moves_the_carrier_without_moving_its_yarn_attachment() {
    let c1 = vec![CarrierId("1".into())];
    let c2 = vec![CarrierId("2".into())];
    let f = |n| Needle::new(Bed::Front, n);
    let p = program(vec![
        KnitoutOp::In {
            carriers: c1.clone(),
        },
        KnitoutOp::Tuck {
            dir: CourseDir::Plus,
            needle: f(10),
            carriers: c1.clone(),
        },
        KnitoutOp::Miss {
            dir: CourseDir::Plus,
            needle: f(30),
            carriers: c1.clone(),
        },
        KnitoutOp::In {
            carriers: c2.clone(),
        },
        KnitoutOp::Tuck {
            dir: CourseDir::Plus,
            needle: f(20),
            carriers: c2.clone(),
        },
        KnitoutOp::Out {
            carriers: c1.clone(),
        },
        KnitoutOp::Out { carriers: c2 },
    ]);
    let profile = MachineProfile::kniterate_7e();
    let entries = schedule(&p, &profile).unwrap();
    let second_carrier_action = entries.iter().position(|entry| {
        matches!(entry, Entry::Pass(pass) if pass.carrier == 2 && pass.front.contains_key(&20))
    }).unwrap();
    assert!(entries[..second_carrier_action].iter().any(|entry| {
        matches!(entry, Entry::Pass(pass) if pass.carrier == 1
            && pass.dir == CourseDir::Minus && pass.yarn == Some([15, 65])
            && pass.front.is_empty() && pass.back.is_empty())
    }));
    let kc = render(&entries).unwrap();
    check_readback(&kc, &p, &entries, &profile).unwrap();

    let unattached = program(vec![
        KnitoutOp::In {
            carriers: c1.clone(),
        },
        KnitoutOp::Miss {
            dir: CourseDir::Plus,
            needle: f(30),
            carriers: c1.clone(),
        },
        KnitoutOp::Out { carriers: c1 },
    ]);
    assert!(schedule(&unattached, &profile).is_err());
}

#[test]
fn unrepresentable_dials_and_identity_changes_are_not_silently_converted() {
    let profile = MachineProfile::kniterate_7e();
    let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 12, 4, &[6.5]).unwrap();
    let fractional = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
    assert_eq!(
        export_kniterate_kc(&fractional, &profile).unwrap_err().code,
        RefusalCode::UnqualifiedProcess
    );
    let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 12, 4, &[6.0]).unwrap();
    let mut bundle = compile_knit_doc(&doc, &profile, None, None, None).unwrap();
    let mut changed = profile.clone();
    changed.child_float_cap_mm += 1.0;
    assert_eq!(
        export_kniterate_kc(&bundle, &changed).unwrap_err().code,
        RefusalCode::VerificationMismatch
    );
    bundle
        .program
        .ops
        .push(KnitoutOp::Rack { quarter_pitches: 0 });
    assert_eq!(
        export_kniterate_kc(&bundle, &profile).unwrap_err().code,
        RefusalCode::VerificationMismatch
    );
}
