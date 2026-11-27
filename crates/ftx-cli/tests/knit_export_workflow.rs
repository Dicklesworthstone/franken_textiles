//! Saved knit documents must produce identical artifacts through the CLI and portable bindings.
//! All yarn/geometry in these tests is synthetic calibration data, never physical qualification.

use ftx_core::receipt::sha256_hex;
use ftx_export::knit::{export_calibration_swatch_kit, export_knit_document};
use ftx_pattern::knit::{MachineProfile, WearerAgeClass, draft_calibration_swatch};
use ftx_pattern::materials::Evidence;
use serde_json::{Value, json};
use std::{fs, path::PathBuf, process::Command};

fn workspace(label: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "ftx-knit-export-{label}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    fs::create_dir(&path).unwrap();
    path
}

fn run(dir: &std::path::Path, args: &[&str], success: bool) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.success(),
        success,
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn saved_calibration_exports_match_native_and_portable_artifacts() {
    let dir = workspace("parity");
    let created = "2026-11-27T08:00";
    for profile in [
        MachineProfile::kniterate_7e(),
        MachineProfile::brother_kh930_ayab(),
        MachineProfile::hand_knit(4.0),
        MachineProfile::stoll_12e_handoff(),
    ] {
        let cam = (profile.cam_setting_range[0] + profile.cam_setting_range[1]) * 0.5;
        let machine = if profile.id.starts_with("hand-knit-") {
            "hand-knit"
        } else {
            &profile.id
        };
        let doc = draft_calibration_swatch(&profile, "synthetic-yarn", 8, 4, &[cam]).unwrap();
        let source = serde_json::to_string_pretty(&doc).unwrap();
        fs::write(dir.join("saved.knit.json"), &source).unwrap();
        let args = [
            "knit",
            "export",
            "saved.knit.json",
            "--machine",
            machine,
            "--out",
            &profile.id,
            "--created",
            created,
        ];
        let cli = run(&dir, &args, true);
        let native = export_knit_document(&doc, &profile, None, None, None, created).unwrap();
        let portable: Value = serde_json::from_str(
            &ftx_wasm::knit::knit_export_json(
                &json!({
                    "machine": machine,
                    "pattern_json": source,
                    "created": created,
                })
                .to_string(),
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(cli["result"]["cert"], portable["cert"]);
        assert_eq!(cli["result"]["pattern_sha256"], portable["pattern_sha256"]);
        for (name, bytes) in &native.files {
            assert_eq!(
                fs::read(dir.join(&profile.id).join(name)).unwrap(),
                *bytes,
                "{name}"
            );
            let entry = portable["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|entry| entry["name"] == *name)
                .unwrap();
            assert_eq!(entry["sha256"], sha256_hex(bytes));
            assert_eq!(entry["bytes"], bytes.len());
        }
        assert_eq!(portable["hardware_execution_authorized"], false);
        assert_eq!(portable["physical_knitting_validated"], false);
        assert_eq!(portable["fit_claimed"], false);
        // Existing output is never overwritten, even when the new request is otherwise valid.
        let before = fs::read(dir.join(&profile.id).join("cert.json")).unwrap();
        run(&dir, &args, false);
        assert_eq!(
            fs::read(dir.join(&profile.id).join("cert.json")).unwrap(),
            before
        );
    }
}

#[test]
fn refused_saved_documents_leave_no_partial_export_directory() {
    let dir = workspace("refusals");
    let profile = MachineProfile::kniterate_7e();
    let mut doc = draft_calibration_swatch(&profile, "synthetic-yarn", 8, 4, &[7.0]).unwrap();
    doc.wearer_age_class = WearerAgeClass::Adult;
    fs::write(
        dir.join("unqualified.json"),
        serde_json::to_vec(&doc).unwrap(),
    )
    .unwrap();
    let refused = run(
        &dir,
        &["knit", "export", "unqualified.json", "--out", "refused"],
        false,
    );
    assert_eq!(refused["refusal"]["code"], "unqualified_process");
    assert!(!dir.join("refused").exists());

    fs::write(dir.join("malformed.json"), b"{broken").unwrap();
    run(
        &dir,
        &["knit", "export", "malformed.json", "--out", "malformed"],
        false,
    );
    assert!(!dir.join("malformed").exists());

    doc.wearer_age_class = WearerAgeClass::CalibrationSwatchOnly;
    fs::write(dir.join("saved.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    fs::write(dir.join("receipt.json"), b"{}").unwrap();
    run(
        &dir,
        &[
            "knit",
            "export",
            "saved.json",
            "--out",
            "bad-evidence",
            "--drape-receipt",
            "receipt.json",
            "--material-sha256",
            "synthetic",
        ],
        false,
    );
    assert!(!dir.join("bad-evidence").exists());
}

#[test]
fn production_export_consumes_the_selected_qualification_and_rejects_stale_gauge() {
    let dir = workspace("qualification");
    let profile = MachineProfile::kniterate_7e();
    let kit = export_calibration_swatch_kit(
        &profile,
        "synthetic-lot",
        "synthetic-material",
        8,
        4,
        &[7.0],
    )
    .unwrap();
    let mut doc = kit.doc;
    let mut qual = kit.qualification_template;
    doc.wearer_age_class = WearerAgeClass::Adult;
    doc.gauge.relaxed_after_wash = true;
    doc.gauge.thickness_mm = Some(2.0);
    doc.metadata
        .insert("material_sha256".into(), "synthetic-material".into());
    doc.metadata
        .insert("yarn_lot_id".into(), "synthetic-lot".into());
    qual.yarn_lot_id = "synthetic-lot".into();
    qual.was_washed_and_blocked = true;
    qual.wales_per_100mm = doc.gauge.wales_per_100mm;
    qual.courses_per_100mm = doc.gauge.courses_per_100mm;
    qual.thickness_mm = 2.0;
    qual.cam_stitch_setting = 7.0;
    qual.evidence = Evidence::Estimate;
    qual.allow_estimates = true;
    fs::write(dir.join("saved.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    fs::write(dir.join("qual.json"), serde_json::to_vec(&qual).unwrap()).unwrap();
    let cli = run(
        &dir,
        &[
            "knit",
            "export",
            "saved.json",
            "--qual",
            "qual.json",
            "--material-sha256",
            "synthetic-material",
            "--out",
            "qualified",
            "--created",
            "2026-11-27T08:00",
        ],
        true,
    );
    assert_eq!(
        cli["result"]["cert"]["qualification_sha256"],
        qual.canonical_sha256().unwrap()
    );
    assert!(dir.join("qualified/qualification.json").exists());
    assert!(!dir.join("qualified/program.kc").exists());
    assert_eq!(
        cli["result"]["backend_export_refusal"]["code"],
        "unqualified_process"
    );

    // Changing the pattern after measurement must not leave a seemingly certified directory.
    doc.gauge.wales_per_100mm += 1.0;
    fs::write(dir.join("changed.json"), serde_json::to_vec(&doc).unwrap()).unwrap();
    let refused = run(
        &dir,
        &[
            "knit",
            "export",
            "changed.json",
            "--qual",
            "qual.json",
            "--material-sha256",
            "synthetic-material",
            "--out",
            "stale-gauge",
        ],
        false,
    );
    assert_eq!(refused["refusal"]["code"], "verification_mismatch");
    assert!(!dir.join("stale-gauge").exists());
}
