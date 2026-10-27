//! Independently specified two-panel sleeve: each cuff edge is 100 mm, each side is 200 mm.
//! The second panel deliberately has reversed winding. No real wearer or third-party catalog data.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use ftx_core::receipt::sha256_hex;
use serde_json::{Value, json};

const SOURCE: &str =
    include_str!("../../../tests/fixtures/import/synthetic-sleeve.garmentcode.json");
const ANNOTATIONS: &str =
    include_str!("../../../tests/fixtures/import/synthetic-sleeve.annotations.json");

fn workspace() -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!(
        "ftx-import-{}-{stamp}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&path).unwrap();
    std::fs::write(path.join("source.json"), SOURCE).unwrap();
    std::fs::write(path.join("annotations.json"), ANNOTATIONS).unwrap();
    path
}

fn run(dir: &Path, args: &[&str], exit: i32) -> Value {
    let output = Command::new(env!("CARGO_BIN_EXE_ftx"))
        .current_dir(dir)
        .args(args)
        .arg("--json")
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(exit),
        "{args:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

fn read(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn annotated_import_preserves_evidence_and_enters_validation_and_exports() {
    let dir = workspace();
    let result = run(
        &dir,
        &[
            "import",
            "garmentcode",
            "source.json",
            "--annotations",
            "annotations.json",
            "--out",
            "imported",
        ],
        0,
    );
    assert_eq!(result["result"]["status"], "REVIEW_REQUIRED");
    assert_eq!(result["result"]["pieces"], 2);
    assert_eq!(result["result"]["seams"], 2);
    assert_eq!(
        std::fs::read(dir.join("imported/source.garmentcode.json")).unwrap(),
        SOURCE.as_bytes()
    );
    assert_eq!(
        std::fs::read(dir.join("imported/annotations.json")).unwrap(),
        ANNOTATIONS.as_bytes()
    );
    let report = read(&dir.join("imported/import.json"));
    assert_eq!(report["source_sha256"], sha256_hex(SOURCE.as_bytes()));
    assert_eq!(
        report["annotations_sha256"],
        sha256_hex(ANNOTATIONS.as_bytes())
    );
    let portable: Value =
        serde_json::from_str(&ftx_wasm::import_garmentcode(SOURCE, Some(ANNOTATIONS)).unwrap())
            .unwrap();
    assert_eq!(portable["report"], report);
    assert_eq!(
        portable["pattern"],
        read(&dir.join("imported/pattern.json"))
    );
    for (field, file) in [
        ("pattern_json", "pattern.json"),
        ("report_json", "import.json"),
        ("fit_spec_json", "fit_spec.json"),
    ] {
        assert_eq!(
            portable[field].as_str().unwrap().as_bytes(),
            std::fs::read(dir.join("imported").join(file)).unwrap()
        );
    }

    std::fs::write(dir.join("measurements.json"), br#"{"cuff_pass":200}"#).unwrap();
    let flat = run(
        &dir,
        &[
            "validate",
            "imported/pattern.json",
            "--measurements",
            "measurements.json",
            "--fit",
            "imported/fit_spec.json",
        ],
        0,
    );
    assert_eq!(flat["result"]["status"], "CONVERGED");
    for target in ["svg", "dxf", "garmentcode"] {
        let out = format!("export-{target}");
        let exported = run(
            &dir,
            &[
                "export",
                target,
                "imported/pattern.json",
                "--out",
                &out,
                "--created",
                "2026-10-27T00:00",
            ],
            0,
        );
        assert!(!exported["result"]["files"].as_array().unwrap().is_empty());
        // Individual exports do not run acceptance implicitly; check their actual artifacts.
        let checked = run(
            &dir,
            &["check", &out, "--pattern-ref", "imported/pattern.json"],
            0,
        );
        assert_eq!(checked["result"]["status"], "PASS");
        assert!(checked["result"]["total"].as_u64().unwrap() > 0);
        assert_eq!(checked["result"]["passed"], checked["result"]["total"]);
        assert!(checked["result"]["failures"].as_array().unwrap().is_empty());
    }
    let gc = std::fs::read_dir(dir.join("export-garmentcode"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .find(|path| path.to_string_lossy().ends_with(".garmentcode.json"))
        .unwrap();
    let reimported = run(
        &dir,
        &[
            "import",
            "garmentcode",
            gc.to_str().unwrap(),
            "--out",
            "reimported",
        ],
        0,
    );
    assert_eq!(reimported["result"]["pieces"], 2);
    // Export splits seam edges at notches; importing those stitches must preserve all of them.
    assert_eq!(reimported["result"]["seams"], 4);
}

#[test]
fn geometry_only_import_does_not_invent_fit_or_grain_and_refuses_existing_output() {
    let dir = workspace();
    let args = ["import", "garmentcode", "source.json", "--out", "imported"];
    run(&dir, &args, 0);
    let pattern = read(&dir.join("imported/pattern.json"));
    assert!(pattern["pieces"][0].get("grain").is_none());
    assert_eq!(pattern["pieces"][0]["cut"]["qty"], 1);
    assert_eq!(pattern["pieces"][1]["cut"]["qty"], 1);
    assert!(!dir.join("imported/fit_spec.json").exists());
    let portable: Value =
        serde_json::from_str(&ftx_wasm::import_garmentcode(SOURCE, None).unwrap()).unwrap();
    assert!(portable["fit_spec_json"].is_null());
    assert_eq!(
        portable["pattern_json"].as_str().unwrap().as_bytes(),
        std::fs::read(dir.join("imported/pattern.json")).unwrap()
    );
    let original = std::fs::read(dir.join("imported/import.json")).unwrap();
    let refused = run(&dir, &args, 3);
    assert_eq!(refused["refusal"]["code"], "invalid_document");
    assert_eq!(
        std::fs::read(dir.join("imported/import.json")).unwrap(),
        original
    );
}

#[test]
fn invalid_external_units_and_annotations_publish_nothing() {
    let dir = workspace();
    let mut source: Value = serde_json::from_str(SOURCE).unwrap();
    source["properties"]["units_in_meter"] = json!(0);
    std::fs::write(dir.join("bad.json"), source.to_string()).unwrap();
    run(
        &dir,
        &["import", "garmentcode", "bad.json", "--out", "bad-units"],
        3,
    );
    assert!(!dir.join("bad-units").exists());
    let mut annotations: Value = serde_json::from_str(ANNOTATIONS).unwrap();
    annotations["panels"]["outer"]["cut"] = json!({"qty":2});
    std::fs::write(dir.join("bad-annotations.json"), annotations.to_string()).unwrap();
    run(
        &dir,
        &[
            "import",
            "garmentcode",
            "source.json",
            "--annotations",
            "bad-annotations.json",
            "--out",
            "bad-annotations",
        ],
        3,
    );
    assert!(!dir.join("bad-annotations").exists());
    assert!(ftx_wasm::import_garmentcode("{", None).is_err());
}

#[test]
fn portable_import_downloads_preserve_signed_zero_and_native_float_bytes() {
    let dir = workspace();
    let mut source: Value = serde_json::from_str(SOURCE).unwrap();
    source["pattern"]["panels"]["outer"]["vertices"][0][0] = json!(-0.0);
    source["pattern"]["panels"]["outer"]["vertices"][1][0] = json!(10.000_000_000_000_002);
    let mut annotations: Value = serde_json::from_str(ANNOTATIONS).unwrap();
    annotations["fit"]["points"][0]["target_mm"] = json!([-0.0, 0.0, 0.125_000_000_000_000_03]);
    let source = source.to_string();
    let annotations = annotations.to_string();
    std::fs::write(dir.join("numeric-source.json"), &source).unwrap();
    std::fs::write(dir.join("numeric-annotations.json"), &annotations).unwrap();
    run(
        &dir,
        &[
            "import",
            "garmentcode",
            "numeric-source.json",
            "--annotations",
            "numeric-annotations.json",
            "--out",
            "numeric-import",
        ],
        0,
    );
    let portable: Value =
        serde_json::from_str(&ftx_wasm::import_garmentcode(&source, Some(&annotations)).unwrap())
            .unwrap();
    for (field, file) in [
        ("pattern_json", "pattern.json"),
        ("report_json", "import.json"),
        ("fit_spec_json", "fit_spec.json"),
    ] {
        assert_eq!(
            portable[field].as_str().unwrap().as_bytes(),
            std::fs::read(dir.join("numeric-import").join(file)).unwrap()
        );
    }
    let pattern: Value = serde_json::from_str(portable["pattern_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        pattern["pieces"][0]["vertices"]["v0000"][0]
            .as_f64()
            .unwrap()
            .to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        pattern["pieces"][0]["vertices"]["v0001"][0]
            .as_f64()
            .unwrap()
            .to_bits(),
        (10.000_000_000_000_002_f64 * 10.0).to_bits()
    );
    let fit: Value = serde_json::from_str(portable["fit_spec_json"].as_str().unwrap()).unwrap();
    assert_eq!(
        fit["points"][0]["target_mm"][0].as_f64().unwrap().to_bits(),
        (-0.0_f64).to_bits()
    );
    assert_eq!(
        fit["points"][0]["target_mm"][2].as_f64().unwrap().to_bits(),
        0.125_000_000_000_000_03_f64.to_bits()
    );
    assert_eq!(
        portable["report"]["source_sha256"],
        sha256_hex(source.as_bytes())
    );
    assert_eq!(
        portable["report"]["annotations_sha256"],
        sha256_hex(annotations.as_bytes())
    );
}

#[test]
fn portable_import_budgets_cover_original_inputs_and_all_download_strings() {
    let oversized = " ".repeat(ftx_pattern::garmentcode_import::MAX_JSON_BYTES + 1);
    for result in [
        ftx_wasm::import_garmentcode(&oversized, None),
        ftx_wasm::import_garmentcode(SOURCE, Some(&oversized)),
    ] {
        assert_eq!(
            result.unwrap_err().code,
            ftx_core::RefusalCode::BudgetExceeded
        );
    }
    // This qualified input remains below 4 MiB. Repeated JSON escaping of its retained fit label
    // expands the complete browser envelope beyond 16 MiB; downloads share the original bound.
    let mut annotations: Value = serde_json::from_str(ANNOTATIONS).unwrap();
    annotations["fit"]["points"][0]["label"] = json!("\"".repeat(1_800_000));
    let annotations = annotations.to_string();
    assert!(annotations.len() < ftx_pattern::garmentcode_import::MAX_JSON_BYTES);
    let refused = ftx_wasm::import_garmentcode(SOURCE, Some(&annotations)).unwrap_err();
    assert_eq!(refused.code, ftx_core::RefusalCode::BudgetExceeded);
    assert!(refused.message.contains("output exceeds 16 MiB"));
}
