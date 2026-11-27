//! A complete synthetic pullover through the actual CLI, with refusal before output creation.

use std::path::Path;
use std::process::Command;

use ftx_pattern::knit::{KnitMachineCert, MachineProfile, SeamedPulloverDraft};
use serde_json::{Value, json};

#[test]
fn pullover_writes_four_checked_pieces_and_keeps_failed_rounds_unpublished() {
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("ftx-knit-pullover-{}-{nonce}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let spec_path = root.join("spec.json");
    let gauge_path = root.join("gauge.json");
    let profile = MachineProfile::hand_knit(4.0);
    let mut spec = json!({
        "name":"synthetic-cli-pullover", "wearer_age_class":"child3_to14",
        "chest_girth_mm":150.0, "hem_girth_mm":140.0,
        "body_length_mm":80.0, "armhole_depth_mm":24.0,
        "sleeve_length_mm":60.0, "cuff_girth_mm":30.0,
        "neck_width_mm":25.0, "front_neck_depth_mm":0.0, "back_neck_depth_mm":0.0,
        "body_rib_courses":0, "cuff_rib_courses":0,
        "cam_setting":0.5 * (profile.cam_setting_range[0] + profile.cam_setting_range[1]),
        "yarn_id":"synthetic-test-yarn", "carrier_id":"1",
        "max_seam_length_mismatch_mm":1.0, "has_hood_or_neck_drawstring":false
    });
    let gauge = json!({
        "wales_per_100mm":20.0, "courses_per_100mm":25.0,
        "needle_pitch_mm":profile.needle_pitch_mm,
        "yarn_tex":180.0, "thickness_mm":2.0, "relaxed_after_wash":true
    });
    std::fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
    std::fs::write(&gauge_path, serde_json::to_vec(&gauge).unwrap()).unwrap();
    let invoke = |out: &Path| {
        Command::new(env!("CARGO_BIN_EXE_ftx"))
            .args(["knit", "pullover"])
            .arg(&spec_path)
            .arg("--gauge")
            .arg(&gauge_path)
            .args([
                "--machine",
                "hand-knit",
                "--created",
                "2026-11-27T12:00",
                "--out",
            ])
            .arg(out)
            .arg("--json")
            .output()
            .unwrap()
    };
    let out = root.join("complete");
    let result = invoke(&out);
    assert!(
        result.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&result.stdout),
        String::from_utf8_lossy(&result.stderr)
    );
    let saved: SeamedPulloverDraft =
        serde_json::from_slice(&std::fs::read(out.join("garment.knit.json")).unwrap()).unwrap();
    saved.validate().unwrap();
    assert_eq!(saved.pieces.len(), 4);
    assert_eq!(saved.seams.len(), 10);
    for piece in &saved.pieces {
        let directory = out.join(piece.role.as_str());
        for filename in [
            "pattern.knit.json",
            "program.k",
            "chart.svg",
            "chart.pdf",
            "instructions.txt",
        ] {
            assert!(directory.join(filename).is_file());
        }
        let cert: KnitMachineCert =
            serde_json::from_slice(&std::fs::read(directory.join("cert.json")).unwrap()).unwrap();
        cert.verify_integrity().unwrap();
        assert!(!cert.hardware_execution_authorized);
    }
    let assembly: Value =
        serde_json::from_slice(&std::fs::read(out.join("assembly.json")).unwrap()).unwrap();
    assert_eq!(assembly["fit_claimed"], false);
    assert!(
        !invoke(&out).status.success(),
        "completed output cannot be overwritten"
    );
    spec["max_seam_length_mismatch_mm"] = json!(0.5);
    std::fs::write(&spec_path, serde_json::to_vec(&spec).unwrap()).unwrap();
    let refused = root.join("refused");
    assert!(!invoke(&refused).status.success());
    assert!(
        !refused.exists(),
        "invalid seam ease must refuse before publication"
    );
}
